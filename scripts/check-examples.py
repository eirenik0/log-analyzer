#!/usr/bin/env python3
"""Check maintained commands and multi-step investigations against one binary."""
import argparse
from copy import deepcopy
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import time
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def pointer(value, path):
    if not path:
        return value
    require(path.startswith('/'), f'invalid JSON pointer: {path}')
    for part in path[1:].split('/'):
        part = part.replace('~1', '/').replace('~0', '~')
        value = value[int(part)] if isinstance(value, list) else value[part]
    return value


def digest(value):
    return hashlib.sha256(json.dumps(value, separators=(',', ':'), sort_keys=True, ensure_ascii=False).encode()).hexdigest()


class BudgetExhausted(Exception):
    pass


class Runner:
    def __init__(self, binary, max_calls=80, max_bytes=2_000_000, max_pages=64):
        self.binary = str(Path(binary).resolve())
        self.environment = {k: v for k, v in os.environ.items() if not k.startswith('LOG_ANALYZER_')}
        self.calls = []
        self.max_calls, self.max_bytes, self.max_pages = max_calls, max_bytes, max_pages
        self.output_bytes = 0

    def invoke(self, args, expected_exit=0):
        if len(self.calls) >= self.max_calls or self.output_bytes >= self.max_bytes:
            raise BudgetExhausted('tool_call_or_output_budget')
        started = time.monotonic()
        result = subprocess.run([self.binary, *args], capture_output=True, env=self.environment, cwd=ROOT, timeout=30)
        self.output_bytes += len(result.stdout)
        call = {'args': args, 'exit': result.returncode, 'elapsed_ms': round((time.monotonic() - started) * 1000, 3)}
        self.calls.append(call)
        require(result.returncode == expected_exit, f'{args}: exit {result.returncode}, expected {expected_exit}: {result.stderr.decode(errors="replace")}')
        value = json.loads(result.stdout)
        require(isinstance(value, dict), 'report must be an object')
        call['report'] = value
        if self.output_bytes > self.max_bytes:
            raise BudgetExhausted('output_budget')
        return value

    def retrieve(self, args, expected_exit=0):
        combined, previous, identity, totals = None, 0, None, None
        consumed = {}
        for _ in range(self.max_pages):
            page = self.invoke(args, expected_exit)
            retrieval = page.get('retrieval')
            require(retrieval is not None, 'workflow requires common bounded retrieval')
            if retrieval['metadata_over_budget'] or retrieval['status'] not in ['page', 'complete']:
                raise BudgetExhausted(retrieval['status'] if not retrieval['metadata_over_budget'] else 'metadata_over_budget')
            current = (retrieval['report_sha256'], page['report_metadata']['evidence']['snapshot_id'], page['report_metadata']['evidence']['profile_sha256'], page['report_metadata']['evidence']['query_sha256'])
            collections = [(c['path'], c['total']) for c in retrieval['collections']]
            require(retrieval['prior_items'] == previous, 'skipped or overlapping global page')
            require(retrieval['total_items'] == sum(c['total'] for c in retrieval['collections']), 'global total mismatch')
            for field, global_field in [('prior', 'prior_items'), ('displayed', 'displayed_items'), ('remaining', 'remaining_items')]:
                require(sum(c[field] for c in retrieval['collections']) == retrieval[global_field], 'collection/global count mismatch')
            for collection in retrieval['collections']:
                path = collection['path']
                require(collection['prior'] == consumed.get(path, 0), 'skipped or overlapping collection page')
                require(collection['prior'] + collection['displayed'] + collection['remaining'] == collection['total'], 'collection count mismatch')
                incoming = pointer(page, path)
                actual = int(bool(incoming)) if isinstance(incoming, str) else len(incoming)
                require(actual == collection['displayed'], 'displayed collection size mismatch')
                consumed[path] = collection['prior'] + collection['displayed']
            if combined is None:
                combined, identity, totals = deepcopy(page), current, collections
            else:
                require(current == identity and collections == totals, 'snapshot/query/collection identity changed during retrieval')
                for collection in retrieval['collections']:
                    target, incoming = pointer(combined, collection['path']), pointer(page, collection['path'])
                    if isinstance(target, list):
                        target.extend(incoming)
                    elif isinstance(target, dict):
                        require(not set(target).intersection(incoming), 'collection keys repeated between pages')
                        target.update(incoming)
                    elif isinstance(target, str):
                        parent_path, key = collection['path'].rsplit('/', 1)
                        parent = pointer(combined, parent_path)
                        parent[key] += incoming
                    else:
                        raise AssertionError('unsupported collection shape')
            advanced = retrieval['prior_items'] + retrieval['displayed_items']
            cursor = retrieval['next_cursor']
            if cursor is None:
                require(retrieval['remaining_items'] == 0, 'retrieval ended with omitted items')
                require(retrieval['status'] == 'complete', 'missing cursor before completion')
                for collection in retrieval['collections']:
                    value = pointer(combined, collection['path'])
                    actual = int(bool(value)) if isinstance(value, str) else len(value)
                    require(actual == collection['total'], 'reconstructed collection incomplete')
                combined.pop('retrieval')
                return combined
            if advanced <= previous or retrieval['displayed_items'] == 0:
                raise BudgetExhausted('no_retrieval_progress')
            require(re.fullmatch(r'[a-f0-9]{64}:\d+', cursor), 'invalid cursor shape')
            require(retrieval['status'] == 'page' and retrieval['remaining_items'] > 0, 'cursor without remaining page')
            require(int(cursor.rsplit(':', 1)[1]) == advanced, 'cursor offset mismatch')
            previous = advanced
            # Only a literal argument changes. Report/log text never becomes a command.
            args = [*args[:args.index('--report-cursor')]] if '--report-cursor' in args else list(args)
            args.extend(['--report-cursor', cursor])
        raise BudgetExhausted('page_budget')


def compatible(capabilities):
    require(capabilities.get('schema_version') == 1, 'unsupported capability schema')
    require(capabilities.get('report_schemas', {}).get('evidence_contract_version') == 1, 'unsupported evidence contract')
    require(capabilities.get('bounded_reports', {}).get('version') == 1, 'bounded retrieval unavailable')
    require(capabilities.get('profile_validation', {}).get('version') == 1, 'profile validation unavailable')
    required = {'info', 'errors', 'validate-profile', 'perf', 'trace', 'search', 'compare'}
    require(required.issubset(capabilities.get('commands', [])), 'required workflow command unavailable')


def check_reference(ref, report):
    evidence = report['report_metadata']['evidence']
    inputs = [i for i in evidence['inputs'] if i['input_id'] == ref['input_id']]
    require(inputs, 'citation input is absent from this snapshot')
    require(ref['location_redacted'] is False, 'citation location unavailable after redaction')
    require(ref['row_path'] is None and ref.get('expansion') is None, 'example citation address type unsupported')
    source = inputs[0]
    path = Path(source['file'])
    require(path.is_absolute() and path.is_relative_to(ROOT / 'examples'), 'unexpected example input path')
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == source['sha256'], 'citation source snapshot changed')
    require(digest([source['file'], source['sha256']]) == source['input_id'], 'citation input identity mismatch')
    require(isinstance(ref['line'], int) and 1 <= ref['line'] <= len(raw.splitlines()), 'citation physical line does not resolve')
    require(digest([ref['input_id'], ref['line'], ref['row_path'], ref.get('expansion')]) == ref['reference_id'], 'citation reference digest invalid')
    records = report.get('evidence_records', [])
    require(any(r['evidence_ref'] == ref for r in records), 'citation is absent from retrieved source records')


def observation(claim, refs, kind='observation'):
    return {'kind': kind, 'claim': claim, 'supporting_refs': refs}


def measurement(claim, operation, report):
    refs = [operation['start_source']['evidence_ref'], operation['end_source']['evidence_ref']]
    return {'kind': 'measurement', 'claim': claim, 'supporting_refs': refs, 'value': operation['duration_ms'], 'unit': 'ms', 'boundaries': {'start': refs[0], 'end': refs[1]}, 'profile_sha256': report['report_metadata']['evidence']['profile_sha256'], 'semantics': 'elapsed_profile_classified_lifecycle'}


def unknown(claim, reason, refs=None):
    return {'kind': 'unknown', 'claim': claim, 'reason': reason, 'supporting_refs': refs or []}


def contract(report, findings, status):
    evidence = report['report_metadata']['evidence']
    for finding in findings:
        for ref in finding['supporting_refs']:
            check_reference(ref, report)
    return {'contract_version': 1, 'input_snapshot_id': evidence['snapshot_id'], 'profile_sha256': evidence['profile_sha256'], 'findings': findings, 'status': status}


def operation(report, name, scope=None):
    matches = [o for o in report['operations'] if o['name'] == name and (scope is None or o['scope'] == [scope])]
    require(len(matches) == 1, f'exact operation is not unique: {name}/{scope}')
    return matches[0]


def parse_time(value):
    # Python 3.10 does not accept the UTC Z spelling emitted by the CLI.
    return datetime.fromisoformat(value[:-1] + '+00:00' if value.endswith('Z') else value)


def validate_step_scope(reports):
    snapshots, profiles = {}, set()
    for report in reports.values():
        evidence = report['report_metadata']['evidence']
        paths = tuple(source['file'] for source in evidence['inputs'])
        identity = (evidence['snapshot_id'], evidence['profile_sha256'])
        require(paths not in snapshots or snapshots[paths] == identity, 'cross-step snapshot/profile changed')
        snapshots[paths] = identity
        profiles.add(evidence['profile_sha256'])
    require(len(profiles) <= 1, 'independent runs use different profiles')


def findings_for(example, reports):
    validate_step_scope(reports)
    kind = example['id']
    for name, report in reports.items():
        if 'validation' in name and kind in ['failure-triage', 'slow-info-comparison', 'reused-id-lifecycle']:
            require(report['profile_validation']['suitability']['status'] == 'supported', 'timing profile not validated')
    if kind == 'failure-triage':
        r = reports['timing']
        op = operation(r, 'lookup')
        require(op['duration_ms'] == 2000 and op['end_classification']['semantics']['outcome'] == 'failure', 'failure lifecycle mismatch')
        require(reports['errors']['errors']['summary']['error_count'] == 1, 'failure inventory mismatch')
        trace = reports['discovery']['trace']['entries']
        require(len(trace) == 3, 'substring collision discovery was lost')
        contextual = next(r for r in r['evidence_records'] if r['source_line_number'] == 2)
        require(contextual['classification']['status'] == 'unclassified' and contextual['structured_fields']['id'] == 'request-70', 'exact identity verification did not reject substring-only context')
        end = op['end_source']['evidence_ref']
        facts = [observation('The lookup lifecycle ended with an observed failure.', [end]), measurement('The matched lookup lifecycle elapsed 2000 ms.', op, r), observation('Substring discovery also selected a different ID and instruction-like context.', [contextual['evidence_ref']], 'contrary_evidence'), unknown('The cause of the failure is unknown.', 'The supplied records contain no causal telemetry.', [end])]
        return [contract(r, facts, 'insufficient_evidence')], None
    if kind == 'slow-info-comparison':
        slow, fast = reports['slow-timing'], reports['baseline-timing']
        slow_op, fast_op = operation(slow, 'run'), operation(fast, 'run')
        a, b = operation(slow, 'worker-a'), operation(slow, 'worker-b')
        require(slow_op['duration_ms'] == 8000 and fast_op['duration_ms'] == 1000, 'run duration mismatch')
        require(parse_time(b['start_time']) < parse_time(a['end_time']), 'parallel work witness missing')
        require(reports['errors']['errors']['summary']['error_count'] == 0, 'INFO-only inventory mismatch')
        entries = reports['slow-discovery']['trace']['entries']
        require(entries[-1]['delta_ms'] == 4000, 'observed unexplained gap mismatch')
        refs = [entries[-2]['evidence_ref'], entries[-1]['evidence_ref']]
        slow_facts = [measurement('Slow run lifecycle elapsed 8000 ms despite an empty ERROR inventory.', slow_op, slow), measurement('Worker A lifecycle elapsed 2000 ms.', a, slow), measurement('Worker B lifecycle elapsed 2000 ms.', b, slow), observation('The worker intervals overlap; their durations are not additive critical-path work.', [a['start_source']['evidence_ref'], a['end_source']['evidence_ref'], b['start_source']['evidence_ref'], b['end_source']['evidence_ref']]), unknown('The cause of the 4000 ms gap after the last worker end is unknown.', 'Trace delta measures an interval between observations, without attributing work or sleep.', refs)]
        contracts = [contract(slow, slow_facts, 'insufficient_evidence'), contract(fast, [measurement('Baseline run lifecycle elapsed 1000 ms.', fast_op, fast)], 'supported')]
        comparison = {'slow_snapshot_id': contracts[0]['input_snapshot_id'], 'baseline_snapshot_id': contracts[1]['input_snapshot_id'], 'elapsed_difference_ms': slow_op['duration_ms'] - fast_op['duration_ms'], 'semantics': 'difference_of_two_independent_observed_lifecycle_intervals', 'causal_attribution': 'unknown'}
        require(comparison['slow_snapshot_id'] != comparison['baseline_snapshot_id'], 'independent runs incorrectly share a snapshot')
        return contracts, comparison
    if kind == 'reused-id-lifecycle':
        r = reports['timing']
        a, b = operation(r, 'lookup', 'a'), operation(r, 'lookup', 'b')
        require(a['correlation_id'] == b['correlation_id'] and a['duration_ms'] == 2000 and b['duration_ms'] == 3000, 'scoped reused-ID timing mismatch')
        return [contract(r, [measurement('Scope a elapsed 2000 ms.', a, r), measurement('Scope b elapsed 3000 ms.', b, r), unknown('A shared ID alone does not identify a lifecycle.', 'Exact scope and classified boundaries distinguish the two observations.')], 'supported')], None
    if kind in ['truncated-capture', 'unsuitable-profile', 'unsupported-input']:
        r = reports['validation']
        expected = {
            'truncated-capture': ('insufficient_evidence', 'identity_scope_boundary_or_capture_evidence_insufficient'),
            'unsuitable-profile': ('unsupported', 'requested_lifecycle_not_recognized'),
            'unsupported-input': ('insufficient_evidence', 'unparsed_input'),
        }[kind]
        suitability = r['profile_validation']['suitability']
        require((suitability['status'], suitability['reason']) == expected, 'abstention basis mismatch')
        refs = [r['evidence_records'][0]['evidence_ref']] if r.get('evidence_records') else []
        status = 'unsupported_input' if kind == 'unsupported-input' else 'insufficient_evidence'
        return [contract(r, [unknown('The supplied evidence cannot establish lifecycle timing or a root cause.', kind, refs)], status)], None
    raise AssertionError(f'unknown maintained workflow: {kind}')


def skill_commands(example, document):
    """Bind the published command sequence to the fixture's semantic assertions."""
    blocks = re.findall(r'^```sh\n(.*?)^```$', document, re.MULTILINE | re.DOTALL)
    commands = [shlex.split(line) for block in blocks for line in block.replace('\\\n', '').splitlines() if line.strip()]
    expected = [['log-analyzer', *args] for args in example['skill_steps']]
    require(commands == expected, f'{example["id"]}: skill example commands differ from checked workflow')
    return commands


def run_primary_skill(binary, commands):
    runner, saved = Runner(binary), {}
    with tempfile.TemporaryDirectory() as directory:
        for command in commands:
            require(command[0] == 'log-analyzer', 'unsupported primary skill executable')
            args = command[1:]
            if 'investigate' in args:
                destination = args[args.index('--artifact') + 1]
                path = str(Path(directory) / Path(destination).name)
                args = [path if arg == destination else arg for arg in args]
                report = runner.invoke(args)
                require(report['artifact']['status'] == 'complete', 'primary skill artifact incomplete')
                raw = Path(path).read_bytes()
                sha = hashlib.sha256(raw).hexdigest()
                require(sha == report['artifact']['stored_sha256'], 'primary skill artifact checksum mismatch')
                saved[destination] = path, sha, json.loads(raw)
            else:
                require(args[0] == 'evidence' and args[1] in saved, 'primary skill retrieval precedes calculation')
                path, sha, artifact = saved[args[1]]
                args[1] = path
                args[args.index('--expected-sha256') + 1] = sha
                collection = args[args.index('--collection') + 1][1:]
                consumed, items = 0, artifact[collection]
                for _ in range(runner.max_pages):
                    page = runner.invoke(args)['artifact_retrieval']
                    require(page['artifact_sha256'] == sha and page['parse_passes'] == page['correlation_passes'] == 0, 'primary skill retrieval changed artifact or repeated analysis')
                    require(page['prior'] == consumed and page['total'] == len(items), 'primary skill retrieval count mismatch')
                    require(page['items'] == items[consumed:consumed + page['displayed']], 'primary skill retrieval changed retained facts')
                    consumed += page['displayed']
                    if page['next_cursor'] is None:
                        require(consumed == len(items), 'primary skill omitted retained facts')
                        break
                    require(page['displayed'] > 0, 'primary skill retrieval made no progress')
                    args = args[:args.index('--report-cursor')] if '--report-cursor' in args else args
                    args += ['--report-cursor', page['next_cursor']]
                else: raise BudgetExhausted('primary_skill_page_budget')
    return {'tool_calls': runner.calls, 'output_bytes': runner.output_bytes}


def run_workflow(binary, example):
    runner = Runner(binary, **example.get('budgets', {}))
    reports, stop = {}, None
    commands = [step['args'] for step in example['steps']]
    primary = None
    if example.get('skill_example'):
        document = (ROOT / example['skill_example']).read_text(encoding='utf-8')
        primary = run_primary_skill(binary, skill_commands(example, document))
    try:
        for step, command in zip(example['steps'], commands):
            args = [a.replace('{root}', str(ROOT)) for a in command]
            # Resolve documented fixture paths for the same absolute citation checks.
            args = [str(ROOT / a) if a.startswith('examples/') else a for a in args]
            report = runner.retrieve(args, step.get('exit', 0))
            for check in step.get('checks', []):
                require(pointer(report, check['path']) == check['value'], f'{example["id"]}/{step["id"]}: {check["path"]} mismatch')
            reports[step['id']] = report
    except BudgetExhausted as error:
        stop = str(error)
    if example.get('expected_stop'):
        require(stop == example['expected_stop'], f'expected explicit budget stop: {stop}')
        require(runner.calls, 'budget example must retrieve its initial scope')
        last = runner.calls[-1]['report']
        investigations, comparison = [contract(last, [unknown('Investigation stopped before enough evidence was retrieved.', stop)], 'budget_exhausted')], None
    else:
        require(stop is None, f'unexpected exhausted workflow: {stop}')
        investigations, comparison = findings_for(example, reports)
    return {'id': example['id'], 'status': 'pass', 'stop_reason': stop, 'tool_calls': runner.calls, 'investigations': investigations, 'comparison': comparison, 'primary_unified': primary}


def run(binary, report_path=None):
    preflight = Runner(binary)
    capabilities = preflight.invoke(['capabilities'])
    compatible(capabilities)
    fixture = str(ROOT / 'examples/synthetic.jsonl')
    for example in json.loads((ROOT / 'examples/commands.json').read_text(encoding='utf-8')):
        args = [a.replace('{fixture}', fixture) for a in example['args']]
        result = subprocess.run([preflight.binary, *args], capture_output=True, text=True, check=True, env=preflight.environment, cwd=ROOT, timeout=30, encoding='utf-8')
        if example.get('type') == 'toml':
            require('profile_name = "example-profile"' in result.stdout and '# Build: log-analyzer' in result.stdout, 'generated profile example mismatch')
        elif example.get('type') == 'version':
            require(result.stdout.startswith('log-analyzer ') and len(result.stdout) < 100, 'version example mismatch')
        else:
            pointer(json.loads(result.stdout), example['pointer'])
        print('Passed:', ' '.join(example['args']))
    manifest = json.loads((ROOT / 'examples/workflows.json').read_text(encoding='utf-8'))
    require(manifest['version'] == 1, 'unsupported workflow manifest')
    workflows = []
    for example in manifest['workflows']:
        workflows.append(run_workflow(binary, example))
        print('Passed workflow:', example['id'])
    result = {'version': 1, 'kind': 'deterministic_workflow_checks', 'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'build': capabilities['build'], 'capabilities': capabilities, 'workflows': workflows, 'limitations': ['These are deterministic example checks, not model-quality or injection-resistance measurements.', 'Schema conformance is validated by the Rust integration test; semantic expectations and reference resolution are checked here.']}
    if report_path:
        path = Path(report_path)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary')
    parser.add_argument('--report', type=Path)
    arguments = parser.parse_args()
    run(arguments.binary, arguments.report)
