"""Fixture-limited deterministic participant; no model reasoning/quality claims."""
from collections import defaultdict
from investigation import require, timestamp, workflow


class BrokerRunner(workflow.Runner):
    def __init__(self, tools, group):
        super().__init__(tools.binary, max_pages=64)
        self.tools, self.group = tools, group

    def invoke(self, args, expected_exit=0):
        return self.tools.invoke({'group': self.group, 'tool': 'analyzer', 'command': args[0], 'options': args[1:]})['report']


def execute(tools, public):
    observations = {}
    for group in public['groups']:
        if tools.arm == 'analyzer':
            reader = BrokerRunner(tools, group)
            info = reader.retrieve(['info'])
            lifecycle = any(task['group'] == group and task['predicate'] in {'elapsed_ms', 'outcome', 'overlap', 'gap_ms', 'cause', 'completion', 'timing_supported', 'identity_unique', 'location_resolvable'} for task in public['tasks'])
            perf = reader.retrieve(['perf', '--op-type', 'request']) if lifecycle and info.get('evidence_records') else {'operations': []}
            if lifecycle: reader.retrieve(['validate-profile', '--kind', 'request'])
            rows = [{'fields': {**r['structured_fields'], 'timestamp': r['timestamp'], 'level': r['level'], 'message': r['message']}, 'ref': r['evidence_ref']} for r in info.get('evidence_records', [])]
            operations = [{'name': o['name'], 'scope': o['scope'][0] if len(o['scope']) == 1 else None, 'id': o['correlation_id'], 'duration': o['duration_ms'], 'start': o['start_source']['evidence_ref'], 'end': o['end_source']['evidence_ref'], 'start_time': o['start_time'], 'end_time': o['end_time'], 'outcome': o['end_classification']['semantics']['outcome']} for o in perf['operations']]
        else:
            rows = []
            for index in range(len(tools.contexts[group]['inputs'])):
                offset = 0
                while True:
                    result = tools.invoke({'group': group, 'tool': 'read', 'input': index, 'offset': offset})
                    rows.extend(result['records'])
                    if result['next_offset'] is None: break
                    offset = result['next_offset']
            starts, ends = defaultdict(list), defaultdict(list)
            for row in rows:
                field = row['fields'] or {}
                identity = (field.get('operation'), field.get('session'), field.get('id'))
                if field.get('phase') == 'start': starts[identity].append(row)
                elif field.get('phase') == 'end': ends[identity].append(row)
            operations = []
            for identity, candidates in starts.items():
                if len(candidates) != 1 or len(ends[identity]) != 1: continue
                start, end = candidates[0], ends[identity][0]
                if start['ref']['location_redacted'] or end['ref']['location_redacted']: continue
                start_time, end_time = [r['fields'].get('timestamp', r['fields'].get('ts')) for r in (start, end)]
                if timestamp(end_time) < timestamp(start_time): continue
                input_index = next(i for i, source in enumerate(tools.contexts[group]['inputs']) if source['input_id'] == start['ref']['input_id'])
                end_index = next(i for i, source in enumerate(tools.contexts[group]['inputs']) if source['input_id'] == end['ref']['input_id'])
                result = tools.invoke({'group': group, 'tool': 'interval', 'input': input_index, 'end_input': end_index, 'start': [start['ref']['line'], start['ref']['row_path']], 'end': [end['ref']['line'], end['ref']['row_path']]})
                operations.append({'name': identity[0], 'scope': identity[1], 'id': identity[2], 'duration': result['elapsed_ms'], 'start': start['ref'], 'end': end['ref'], 'start_time': start_time, 'end_time': end_time, 'outcome': end['fields'].get('outcome')})
        observations[group] = rows, operations
    facts, statuses = [], {}
    for task in public['tasks']:
        group, subject, predicate = task['group'], task['subject'], task['predicate']
        rows, operations = observations[group]
        name, _, scope = subject.partition('@')
        matches = [o for o in operations if o['name'] == name and (not scope or o['scope'] == scope)]
        op = matches[0] if len(matches) == 1 else None
        refs = [op['start'], op['end']] if op else [r['ref'] for r in rows if r['fields'] and r['fields'].get('operation') == name and r['fields'].get('phase') == 'start']
        fact = {**task, 'kind': 'observation', 'value': None, 'refs': refs}
        if predicate == 'elapsed_ms':
            require(op is not None, 'scripted participant cannot establish exact lifecycle')
            fact.update(kind='measurement', value=op['duration'], boundaries={'start': op['start'], 'end': op['end']})
        elif predicate == 'outcome': fact.update(value=op['outcome'], refs=[op['end']])
        elif predicate == 'overlap':
            workers = [o for o in operations if o['name'].startswith('worker-')]
            require(len(workers) == 2, 'scripted overlap requires two workers')
            fact.update(value=max(timestamp(o['start_time']) for o in workers) < min(timestamp(o['end_time']) for o in workers), refs=[ref for o in workers for ref in (o['start'], o['end'])])
        elif predicate == 'gap_ms':
            worker = max((o for o in operations if o['name'].startswith('worker-')), key=lambda o: timestamp(o['end_time']))
            value = int((timestamp(op['end_time']) - timestamp(worker['end_time'])).total_seconds() * 1000)
            fact.update(kind='measurement', value=value, refs=[worker['end'], op['end']], boundaries={'start': worker['end'], 'end': op['end']})
        elif predicate == 'parsed_entries': fact.update(value=sum(isinstance(r['fields'], dict) for r in rows), refs=[r['ref'] for r in rows if isinstance(r['fields'], dict)])
        elif predicate == 'info_count': fact.update(value=sum((r['fields'] or {}).get('level') == 'INFO' for r in rows), refs=[r['ref'] for r in rows])
        elif predicate == 'lifecycle_present': fact.update(value=any((r['fields'] or {}).get('phase') in {'start', 'end'} for r in rows), refs=[r['ref'] for r in rows])
        elif predicate == 'parsing_supported':
            fact.update(kind='unknown', value=any(isinstance(r['fields'], dict) for r in rows), refs=[])
        elif predicate == 'error_count': fact.update(value=sum((r['fields'] or {}).get('level') == 'ERROR' for r in rows), refs=[r['ref'] for r in rows])
        elif predicate == 'identity_unique':
            same = [o for o in operations if o['id'] == subject]
            fact.update(value=len(same) <= 1, refs=[ref for o in same for ref in (o['start'], o['end'])])
        elif predicate == 'authority':
            context = next(r for r in rows if not (r['fields'] or {}).get('phase') and 'instructions' in (r['fields'] or {}).get('message', ''))
            fact.update(kind='contrary_evidence', value=False, refs=[context['ref']])
        elif predicate == 'location_resolvable':
            require(any(r['ref']['location_redacted'] for r in rows), 'location loss not observed')
            fact.update(kind='unknown', value=False, refs=[])
        elif predicate in {'cause', 'completion', 'timing_supported'}:
            fact.update(kind='unknown', value=False if predicate == 'timing_supported' else 'unknown', refs=refs or [r['ref'] for r in rows])
        else: raise AssertionError('unsupported scripted predicate')
        facts.append(fact)
        statuses[group] = 'unsupported_input' if predicate == 'parsing_supported' and fact['value'] is False else 'insufficient_evidence' if fact['kind'] == 'unknown' or statuses.get(group) == 'insufficient_evidence' else 'supported'
    return {'status': statuses, 'findings': facts}, {'tokens': None, 'provider_cost_usd': None, 'measurement_source': 'unavailable_for_scripted_participant'}
