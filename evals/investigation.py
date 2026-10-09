"""Grounded typed-claim scoring and read-only tools for investigation evaluations."""
from copy import deepcopy
from datetime import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('workflow_examples', ROOT / 'scripts/check-examples.py')
workflow = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workflow)
require, digest, pointer = workflow.require, workflow.digest, workflow.pointer
PREDICATES = {'elapsed_ms', 'outcome', 'overlap', 'gap_ms', 'error_count', 'cause', 'completion', 'timing_supported', 'identity_unique', 'location_resolvable', 'authority', 'parsed_entries', 'info_count', 'lifecycle_present', 'parsing_supported'}
FIELDS = {'group', 'subject', 'predicate', 'kind', 'value', 'refs', 'boundaries'}


def key(fact):
    return fact['group'], fact['subject'], fact['predicate']


def address(ref, context):
    """Resolve original immutable bytes, physical line and supported normalized row."""
    require(isinstance(ref, dict) and set(ref) == {'input_id', 'line', 'row_path', 'reference_id', 'location_redacted'} and ref.get('location_redacted') is False, 'citation location unavailable')
    source = next((s for s in context['inputs'] if s['input_id'] == ref.get('input_id')), None)
    require(source is not None, 'citation input outside declared snapshot')
    path = Path(source['file'])
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == source['sha256'], 'input bytes changed')
    require(digest([str(path), source['sha256']]) == ref['input_id'], 'input identity mismatch')
    line = ref.get('line')
    require(type(line) is int and 1 <= line <= len(raw.splitlines()), 'invalid physical line')
    row = ref.get('row_path')
    require(row is None or isinstance(row, str), 'invalid row pointer')
    require(ref.get('expansion') is None, 'expansion citations unsupported by this corpus')
    if row is not None:
        value = pointer(json.loads(raw.splitlines()[line - 1]), row)
        require(isinstance(value, dict), 'normalized row must resolve to an object')
    require(digest([ref['input_id'], line, row, None]) == ref.get('reference_id'), 'reference digest mismatch')
    return source['label'], line, row


def score(scenario, response, contexts, revealed):
    """No model judge: only strict typed predicates with exact grounded support."""
    errors, checks = [], []
    expected = {key(f): f for f in scenario['expected']}
    received = {}
    if not isinstance(response, dict) or set(response) != {'status', 'findings'} or not isinstance(response.get('findings'), list):
        return {'status': 'FAIL', 'errors': ['invalid typed response envelope'], 'checks': [], 'omissions': list(map(list, expected)), 'unsupported_causal_claims': 0}
    if response['status'] != scenario['status']:
        errors.append('incorrect conclusion/abstention status')
    causal = 0
    for fact in response['findings']:
        try:
            require(isinstance(fact, dict) and set(fact) <= FIELDS and FIELDS - {'boundaries'} <= set(fact), 'unscored or missing claim fields')
            require(fact['predicate'] in PREDICATES, 'unsupported predicate')
            if fact['predicate'] == 'cause' and fact['value'] != 'unknown':
                causal += 1
            identity = key(fact)
            require(identity not in received, 'duplicate fact')
            received[identity] = fact
            require(identity in expected, 'unsupported/extraneous claim')
            truth = expected[identity]
            require(type(fact['value']) is type(truth['value']) and fact['value'] == truth['value'], 'incorrect factual value')
            require(fact['kind'] == truth['kind'], 'incorrect epistemic kind')
            if fact['predicate'] == 'parsing_supported' and fact['value'] is False:
                require('__unsupported__' in revealed[fact['group']], 'unsupported coverage was never observed')
            if fact['predicate'] == 'location_resolvable' and fact['value'] is False:
                require('__location_loss__' in revealed[fact['group']], 'location loss was never observed')
            context = contexts[fact['group']]
            refs = fact['refs']
            require(isinstance(refs, list), 'citation list required')
            coordinates = [address(ref, context) for ref in refs]
            require(len(set(coordinates)) == len(coordinates), 'duplicate source citations')
            require(set(coordinates) == {tuple(a) for a in truth['support']}, 'citation does not support this scoped fact')
            require(all(ref['reference_id'] in revealed[fact['group']] for ref in refs), 'citation evidence never revealed by participant tools')
            if truth['kind'] == 'measurement':
                require(set(fact.get('boundaries', {})) == {'start', 'end'}, 'measurement boundaries missing')
                boundaries = [address(fact['boundaries'][name], context) for name in ('start', 'end')]
                require(boundaries == [tuple(a) for a in truth['boundaries']], 'wrong or reversed measurement pair')
                require(all(fact['boundaries'][name] in refs for name in ('start', 'end')), 'boundary absent from support')
            else:
                require('boundaries' not in fact, 'boundaries on non-measurement')
            checks.append({'identity': list(identity), 'passed': True})
        except (AssertionError, KeyError, TypeError, ValueError, IndexError) as error:
            errors.append(str(error))
            checks.append({'identity': list(key(fact)) if isinstance(fact, dict) and {'group', 'subject', 'predicate'} <= set(fact) else None, 'passed': False, 'error': str(error)})
    omissions = [list(identity) for identity in expected if identity not in received]
    if omissions:
        errors.append('important evidence/unknowns omitted')
    return {'status': 'FAIL' if errors else 'PASS', 'errors': errors, 'checks': checks, 'omissions': omissions, 'unsupported_causal_claims': causal}


def reference(source, line, row=None):
    ref = {'input_id': source['input_id'], 'line': line, 'row_path': row, 'location_redacted': False}
    ref['reference_id'] = digest([ref['input_id'], line, row, None])
    return ref


def timestamp(value):
    require(isinstance(value, str), 'timestamp unavailable for interval script')
    return workflow.parse_time(value)


def profile_sources(profile):
    sources, seen = [], set()
    path = ROOT / profile
    builtins = {'base': 'config/profiles/base.toml', 'eyes': 'config/profiles/eyes.toml', 'service-api': 'config/templates/service-api.toml', 'custom-start': 'config/templates/custom-start.toml', 'event-pipeline': 'config/templates/event-pipeline.toml'}
    while True:
        path = path.resolve()
        require(path.is_relative_to(ROOT) and path not in seen and len(seen) < 8, 'profile inheritance outside fixture scope')
        seen.add(path)
        text = path.read_text()
        sources.append({'file': str(path), 'label': str(path.relative_to(ROOT)), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
        parent = re.search(r'^extends\s*=\s*"([^"\n]+)"', text, re.MULTILINE)
        if not parent: return sources
        name = parent.group(1)
        path = ROOT / builtins[name] if name in builtins else path.parent / name


class Tools:
    """Adapter is trusted executable code; this broker restricts tool requests only."""
    def __init__(self, binary, scenario, arm, contexts, budgets):
        self.binary = str(Path(binary).resolve())
        self.scenario, self.arm, self.contexts, self.budgets = scenario, arm, contexts, budgets
        self.calls, self.output_bytes = [], 0
        self.revealed = {group: set() for group in contexts}
        self.started = time.monotonic()
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('LOG_ANALYZER_')}
        self.env['TZ'] = 'UTC'

    def remaining(self):
        remaining = self.budgets['wall_seconds'] - (time.monotonic() - self.started)
        require(remaining > 0, 'wall budget exhausted')
        return remaining

    def record(self, request, output, started):
        encoded = json.dumps(output, ensure_ascii=False).encode()
        self.output_bytes += len(encoded)
        require(self.output_bytes <= self.budgets['output_bytes'], 'output budget exhausted')
        self.calls.append({'request': request, 'output': output, 'elapsed_ms': round((time.monotonic() - started) * 1000, 3)})
        def collect(value):
            if isinstance(value, dict):
                if {'reference_id', 'input_id', 'line', 'row_path', 'location_redacted'} <= set(value):
                    if value.get('location_redacted') is True:
                        self.revealed[request['group']].add('__location_loss__')
                    # Only independently resolvable source references receive evidence credit.
                    try:
                        address(value, self.contexts[request['group']])
                        self.revealed[request['group']].add(value['reference_id'])
                    except (AssertionError, ValueError, KeyError, IndexError, TypeError):
                        pass
                for child in value.values(): collect(child)
            elif isinstance(value, list):
                for child in value: collect(child)
        collect(output)
        report = output.get('report', {})
        coverage = report.get('coverage', {})
        if coverage.get('status') == 'unparsed_input' or coverage.get('unparsed_files', 0) > 0 or any(source.get('status') == 'unparsed_input' for source in coverage.get('files', [])):
            self.revealed[request['group']].add('__unsupported__')
        if 'records' in output and output['records'] and all(record.get('fields') is None for record in output['records']):
            self.revealed[request['group']].add('__unsupported__')
        return output

    def records(self, source):
        require(hashlib.sha256(Path(source['file']).read_bytes()).hexdigest() == source['sha256'], 'input bytes changed')
        records = []
        for line, raw in enumerate(Path(source['file']).read_text().splitlines(), 1):
            try: row = json.loads(raw)
            except ValueError:
                classic = re.fullmatch(r'[^|]+\| (?P<timestamp>\S+) \[(?P<level>\w+) *\] (?P<message>.*)', raw)
                tracing = re.fullmatch(r'(?P<timestamp>\S+) (?P<level>INFO|ERROR|WARN|DEBUG|TRACE) [^: ]+(?:::[^: ]+)*: (?P<message>.*)', raw)
                match = classic or tracing
                row = match.groupdict() if match else None
                if row is not None:
                    session = re.search(r'\(([^)]+)\) \|', raw)
                    row['session'] = session.group(1) if session else None
                    boundary = re.search(r'Request "([^"\n]+)" \[([^]\n]+)\] (sent|completed)', row['message'])
                    if boundary:
                        row.update(operation=boundary.group(1), id=boundary.group(2), phase='start' if boundary.group(3) == 'sent' else 'end')
            rows = [(f'/rows/{index}', child) for index, child in enumerate(row['rows'])] if isinstance(row, dict) and 'rows' in row else [(None, row)]
            for row_path, value in rows:
                if self.scenario.get('redact') and isinstance(value, dict):
                    value = deepcopy(value)
                    for name in ('token', 'id'):
                        if name in value: value[name] = '[REDACTED]' if name == 'token' else '[ID_' + digest(value[name])[:12] + ']'
                    safe_raw = json.dumps(value)
                else: safe_raw = raw
                ref = reference(source, line, row_path)
                if self.scenario.get('redact_location') and row_path is not None: ref = {**ref, 'row_path': None, 'location_redacted': True}
                records.append({'raw': safe_raw, 'fields': value, 'ref': ref})
        return records

    def invoke(self, request):
        started = time.monotonic()
        try:
            return self._invoke(request)
        except Exception as error:
            self.calls.append({'request': request, 'error': str(error), 'elapsed_ms': round((time.monotonic() - started) * 1000, 3)})
            raise

    def _invoke(self, request):
        started = time.monotonic()
        self.remaining()
        require(len(self.calls) < self.budgets['tool_calls'], 'tool-call budget exhausted')
        require(isinstance(request, dict) and request.get('group') in self.contexts, 'undeclared input group')
        group = request['group']
        inputs = self.contexts[group]['inputs']
        for source in self.contexts[group].get('profile_sources', []):
            require(hashlib.sha256(Path(source['file']).read_bytes()).hexdigest() == source['sha256'], 'profile bytes changed')
        if request.get('tool') == 'profile':
            require(set(request) == {'group', 'tool'}, 'unapproved profile read fields')
            return self.record(request, {'sources': [{**source, 'text': Path(source['file']).read_text()} for source in self.contexts[group]['profile_sources']]}, started)
        if self.arm == 'analyzer':
            require(set(request) <= {'group', 'tool', 'command', 'options'} and request.get('tool') == 'analyzer', 'analyzer arm tool unavailable')
            command = request.get('command')
            require(command in {'info', 'errors', 'perf', 'trace', 'search', 'validate-profile'}, 'read-only command unavailable')
            options = request.get('options', [])
            require(isinstance(options, list) and len(options) % 2 == 0, 'options must be literal flag/value pairs')
            allowed = {'--id', '--session', '--field', '--filter', '--kind', '--purpose', '--op-type', '--report-cursor'}
            for flag, value in zip(options[::2], options[1::2]):
                require(flag in allowed and isinstance(value, str), 'unapproved option')
                if flag == '--report-cursor': require(re.fullmatch(r'[a-f0-9]{64}:\d+', value), 'invalid cursor')
            args = [self.binary, '--config', str(ROOT / self.scenario['profile']), '--report-max-items', '5']
            if self.scenario.get('redact'):
                args += ['--redact', '--mask-id', 'id']
            if self.scenario.get('redact_location'):
                args += (['--redact'] if not self.scenario.get('redact') else []) + ['--mask-id', 'row_path']
            args += [command, *[i['file'] for i in inputs], *options]
            result = subprocess.run(args, capture_output=True, env=self.env, cwd=ROOT, timeout=self.remaining())
            require(result.returncode in {0, 1}, 'analyzer command failed')
            require(len(result.stdout) <= self.budgets['output_bytes'] - self.output_bytes, 'output budget exhausted')
            output = {'exit': result.returncode, 'report': json.loads(result.stdout)}
            evidence = output['report']['report_metadata']['evidence']
            require((evidence['snapshot_id'], evidence['profile_sha256']) == (self.contexts[group]['snapshot_id'], self.contexts[group]['profile_sha256']), 'tool snapshot/profile changed')
        else:
            require(request.get('tool') in {'read', 'search', 'interval'}, 'baseline tool unavailable')
            require(type(request.get('input', 0)) is int and 0 <= request.get('input', 0) < len(inputs), 'input outside scope')
            source = inputs[request.get('input', 0)]
            records = self.records(source)
            if request['tool'] == 'interval':
                require(set(request) <= {'group', 'tool', 'input', 'end_input', 'start', 'end'}, 'unapproved script fields')
                chosen = []
                for name in ('start', 'end'):
                    coordinate = request[name]
                    end_input = request.get('end_input', request.get('input', 0))
                    require(type(end_input) is int and 0 <= end_input < len(inputs), 'end input outside related group')
                    candidates = records if name == 'start' else self.records(inputs[end_input])
                    match = next((r for r in candidates if [r['ref']['line'], r['ref']['row_path']] == coordinate), None)
                    require(match is not None, 'interval boundary outside declared source')
                    require(isinstance(match['fields'], dict), 'interval script requires JSON timestamp rows')
                    chosen.append(match)
                output = {'elapsed_ms': int((timestamp(chosen[1]['fields'].get('timestamp', chosen[1]['fields'].get('ts'))) - timestamp(chosen[0]['fields'].get('timestamp', chosen[0]['fields'].get('ts')))).total_seconds() * 1000), 'boundaries': [r['ref'] for r in chosen]}
            else:
                require(set(request) <= {'group', 'tool', 'input', 'offset', 'needle'}, 'unapproved baseline fields')
                offset = request.get('offset', 0)
                require(type(offset) is int and offset >= 0, 'invalid record offset')
                if request['tool'] == 'search':
                    require(isinstance(request.get('needle'), str), 'literal search needle required')
                    records = [r for r in records if request['needle'] in r['raw']]
                output = {'records': records[offset:offset + 5], 'next_offset': offset + 5 if offset + 5 < len(records) else None, 'total': len(records)}
        return self.record(request, output, started)


def contexts_for(binary, scenario):
    contexts, calls = {}, []
    for group, files in scenario['groups'].items():
        args = [str(binary), '--config', str(ROOT / scenario['profile']), '--report-max-items', '0', 'info', *[str(ROOT / file) for file in files]]
        started = time.monotonic()
        result = subprocess.run(args, capture_output=True, cwd=ROOT, timeout=30, env={**{k: v for k, v in os.environ.items() if not k.startswith('LOG_ANALYZER_')}, 'TZ': 'UTC'})
        require(result.returncode in {0, 1}, 'scope preflight failed')
        evidence = json.loads(result.stdout)['report_metadata']['evidence']
        sources = []
        for file in files:
            path = ROOT / file
            sha = hashlib.sha256(path.read_bytes()).hexdigest()
            sources.append({'file': str(path), 'label': file, 'sha256': sha, 'input_id': digest([str(path), sha])})
        require([s['input_id'] for s in sources] == [s['input_id'] for s in evidence['inputs']], 'input manifest mismatch')
        contexts[group] = {'inputs': sources, 'snapshot_id': evidence['snapshot_id'], 'profile_sha256': evidence['profile_sha256'], 'profile_sources': profile_sources(scenario['profile'])}
        calls.append({'command': args, 'exit': result.returncode, 'output_bytes': len(result.stdout), 'elapsed_ms': round((time.monotonic() - started) * 1000, 3)})
    return contexts, calls
