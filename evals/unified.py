"""Read-only unified broker; joins explicit memberships, never reconstructs pairs."""
import hashlib
import json
import re
import subprocess
import tempfile
import time
from pathlib import Path
from investigation import Tools, ROOT, require, address
from attempt import BudgetExceeded
from schema import validate
from scripted import interpret


class UnifiedTools(Tools):
    contract = {'tools': ['profile', 'investigate', 'evidence'], 'scope': 'declared immutable groups only; one investigation per group', 'evidence': {'collections': ['/records', '/findings', '/populations', '/memberships/N/members'], 'cursor': 'exact next_cursor from the prior artifact page', 'page_items': 5}}
    def __init__(self, binary, scenario, contexts, budgets, schemas):
        super().__init__(binary, scenario, 'unified', contexts, budgets)
        self.schemas = schemas
        self.directory = tempfile.TemporaryDirectory()
        self.saved, self.pages = {}, {}

    def close(self): self.directory.cleanup()

    def _invoke(self, request):
        started = time.monotonic()
        self.remaining()
        if len(self.calls) >= self.budgets['tool_calls']: raise BudgetExceeded('tool-call budget exhausted')
        require(isinstance(request, dict) and request.get('group') in self.contexts, 'undeclared input group')
        group, tool = request['group'], request.get('tool')
        if tool == 'profile': return super()._invoke(request)
        for source in self.contexts[group]['inputs'] + self.contexts[group]['profile_sources']:
            require(hashlib.sha256(Path(source['file']).read_bytes()).hexdigest() == source['sha256'], 'frozen input/profile changed')
        if tool == 'investigate':
            require(set(request) == {'group', 'tool'} and group not in self.saved, 'one investigation per group required')
            require(len(self.contexts[group]['inputs']) == 1, 'unified inputs are independent; related split-file comparison is ineligible')
            artifact = Path(self.directory.name) / (str(len(self.saved)) + '.json')
            args = [self.binary, '--config', str(ROOT / self.scenario['profile']), '--report-max-items', '5']
            if self.scenario.get('redact_location') or self.scenario.get('redact'): args += ['--redact']
            if self.scenario.get('redact_location'): args += ['--mask-id', 'row_path']
            if self.scenario.get('redact'): args += ['--mask-id', 'id']
            args += ['investigate', self.contexts[group]['inputs'][0]['file'], '--artifact', str(artifact)]
        else:
            require(tool == 'evidence' and group in self.saved, 'investigate first; read-only evidence tool required')
            require(set(request) <= {'group', 'tool', 'collection', 'cursor'}, 'unapproved retrieval fields')
            collection = request.get('collection', '/findings')
            require(isinstance(collection, str) and re.fullmatch(r'/(records|findings|populations|memberships/\d+/members)', collection), 'unapproved evidence collection')
            saved = self.saved[group]
            args = [self.binary, 'investigation-evidence', str(saved['path']), '--expected-sha256', saved['sha256'], '--collection', collection, '--report-max-items', '5']
            if 'cursor' in request:
                require(isinstance(request['cursor'], str) and re.fullmatch(r'v1:[a-f0-9]{64}:\d+', request['cursor']), 'invalid artifact cursor')
                args += ['--report-cursor', request['cursor']]
        try:
            result = subprocess.run(args, capture_output=True, cwd=ROOT, env=self.env, timeout=self.remaining())
        except subprocess.TimeoutExpired as error:
            self.output_bytes += len(error.stdout or b'')
            raise
        self.output_bytes += len(result.stdout)
        if self.output_bytes > self.budgets['output_bytes']: raise BudgetExceeded('output budget exhausted')
        require(result.returncode == 0, 'unified command failed')
        output = json.loads(result.stdout)
        if tool == 'investigate':
            validate(output, self.schemas['investigation'])
            manifest = output['report_metadata']['evidence']
            require((manifest['snapshot_id'], manifest['profile_sha256']) == (self.contexts[group]['snapshot_id'], self.contexts[group]['profile_sha256']), 'unified snapshot/profile changed')
            require([i['input_id'] for i in manifest['inputs']] == [i['input_id'] for i in self.contexts[group]['inputs']], 'unified input bindings changed')
            require(output['artifact']['status'] == 'complete' and output['processing']['status'] == 'complete', 'complete local artifact required for this frozen comparison')
            sha = output['artifact']['stored_sha256']
            artifact_bytes = artifact.read_bytes()
            require(hashlib.sha256(artifact_bytes).hexdigest() == sha, 'artifact digest changed')
            retained = json.loads(artifact_bytes)
            validate(retained, self.schemas['evidence_artifact'])
            self.saved[group] = {'path': artifact, 'sha256': sha, 'artifact': retained}
            if manifest['scope']['status'] == 'unparsed_input': self.revealed[group].add('__unsupported__')
        else:
            require(set(output) == {'artifact_retrieval'}, 'invalid retrieval envelope')
            page = output['artifact_retrieval']
            self.validate_page(page, request)
            require(page['artifact_sha256'] == self.saved[group]['sha256'], 'retrieval artifact changed')
            require(page['parse_passes'] == page['correlation_passes'] == 0, 'retrieval repeated engine work')
        return self.record(request, {'report': output}, started, received_bytes=0)

    def validate_page(self, page, request):
        # The current binary advertises artifact/item schemas, not a page
        # envelope schema. These are separately declared broker invariants.
        keys = {'artifact_sha256','binding_sha256','collection','contract_version','correlation_passes','displayed','id','items','next_cursor','parse_passes','prior','remaining','source_verification','status','total'}
        require(isinstance(page, dict) and set(page) == keys, 'unsupported artifact page envelope')
        require(page['contract_version'] == 1 and page['status'] in {'page','complete'} and page['id'] is None, 'unsupported artifact page contract')
        collection, group = request.get('collection', '/findings'), request['group']
        require(page['collection'] == collection, 'page collection changed')
        for name in ('artifact_sha256', 'binding_sha256'):
            require(isinstance(page[name], str) and re.fullmatch('[a-f0-9]{64}', page[name]), 'invalid page digest')
        for name in ('prior','displayed','remaining','total','parse_passes','correlation_passes'):
            require(type(page[name]) is int and page[name] >= 0, 'invalid page count')
        require(isinstance(page['items'], list) and page['displayed'] == len(page['items']), 'displayed count mismatch')
        require(page['prior'] + page['displayed'] + page['remaining'] == page['total'], 'page count partition mismatch')
        previous = self.pages.get((group, collection))
        if 'cursor' in request:
            require(previous is not None and request['cursor'] == previous['next_cursor'], 'cursor not delivered by prior page')
            require(page['binding_sha256'] == previous['binding_sha256'], 'page binding changed')
            require(page['prior'] == previous['prior'] + previous['displayed'], 'page did not advance')
        else:
            require(page['prior'] == 0, 'initial page offset mismatch')
        expected_cursor = f"v1:{page['binding_sha256']}:{page['prior'] + page['displayed']}" if page['remaining'] else None
        require(page['next_cursor'] == expected_cursor and page['status'] == ('page' if page['remaining'] else 'complete'), 'cursor/count mismatch')
        require(not page['remaining'] or page['displayed'] > 0, 'page made no progress')
        verification = page['source_verification']
        require(isinstance(verification, dict) and set(verification) == {'facts','reason','status'} and verification['status'] == 'not_requested' and verification['facts'] == 'retained_snapshot' and isinstance(verification['reason'], str), 'unsupported source verification envelope')
        root = self.schemas['evidence_artifact']
        definition = root['$defs']['retained_record' if collection == '/records' else 'finding' if collection == '/findings' else 'population' if collection == '/populations' else 'population_members']
        if collection.startswith('/memberships/'):
            definition = definition['properties']['members']['items']
        for item in page['items']: validate(item, definition, root)
        retained = self.saved[group]['artifact']
        if collection.startswith('/memberships/'):
            index = int(collection.split('/')[2])
            require(index < len(retained['memberships']), 'unknown retained membership')
            items = retained['memberships'][index]['members']
        else:
            items = retained[collection[1:]]
        require(page['total'] == len(items), 'page total differs from retained collection')
        require(page['items'] == items[page['prior']:page['prior'] + page['displayed']], 'page items differ from retained collection slice')
        self.pages[group, collection] = page


def retrieve(tools, group, collection):
    items, cursor, seen = [], None, set()
    while True:
        request = {'group': group, 'tool': 'evidence', 'collection': collection}
        if cursor is not None: request['cursor'] = cursor
        page = tools.invoke(request)['report']['artifact_retrieval']
        require(page['status'] in {'page','complete'}, 'retrieval unavailable')
        items.extend(page['items'])
        cursor = page['next_cursor']
        if cursor is None: return items
        require(cursor not in seen and page['displayed'] > 0, 'retrieval made no progress')
        seen.add(cursor)


def observations(tools, public):
    result = {}
    for group in public['groups']:
        tools.invoke({'group': group, 'tool': 'investigate'})
        records = retrieve(tools, group, '/records')
        rows = []
        for record in records:
            fields = record['fields']
            require(record['data_omitted'] or 'level' in fields, 'older artifact lacks canonical parsed severity; raw reparsing is unavailable')
            row = None if record['data_omitted'] else {**(fields.get('structured_fields') or {}), 'level': fields['level'], 'timestamp': record['timestamp'], 'message': record['message']}
            rows.append({'fields': row, 'ref': record['occurrence']['evidence_ref']})
        by_ref = {row['ref']['reference_id']: row for row in rows}
        findings = {f['id']: f for f in retrieve(tools, group, '/findings')}
        populations = retrieve(tools, group, '/populations')
        operations = []
        for population in populations:
            if not population['id'].endswith('-paired-lifecycles'): continue
            for member in retrieve(tools, group, population['membership']['collection']):
                if any(record['data_omitted'] for record in records): continue
                if len(member['measurement_ids']) != 1: continue
                measurement = findings[member['measurement_ids'][0]]
                require(measurement['kind'] == 'measurement', 'explicit elapsed measurement required')
                details = measurement['details']
                start, end = [details['boundaries'][phase]['occurrence']['evidence_ref'] for phase in ('start', 'end')]
                require([o['evidence_ref'] for o in member['source_occurrences']] == [start, end], 'membership boundary mismatch')
                require(start['reference_id'] in by_ref and end['reference_id'] in by_ref, 'measurement records not delivered')
                identities = {i['field']: i['value'] for i in member['identity']}
                scope = json.loads(identities['scope'])
                end_record = next(r for r in records if r['occurrence']['evidence_ref']['reference_id'] == end['reference_id'])
                semantics = end_record['fields']['classification']['semantics']
                operations.append({'name': identities['name'], 'scope': scope[0] if len(scope) == 1 else None, 'id': identities['correlation_id'], 'duration': details['value'], 'start': start, 'end': end, 'start_time': details['boundaries']['start']['timestamp'], 'end_time': details['boundaries']['end']['timestamp'], 'outcome': semantics['outcome']})
        result[group] = rows, operations
    return result


def execute(tools, public):
    return interpret(public, observations(tools, public)), None
