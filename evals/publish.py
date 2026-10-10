#!/usr/bin/env python3
"""Strip local paths/raw traces from a synthetic investigation baseline report."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
from attempt import aggregate_usage


def aggregate(report):
    arms = defaultdict(list)
    for record in report['records']:
        arms[record['arm']].append(record)
    return {'version': 2, 'kind': report['kind'], 'binary_sha256': report['binary_sha256'], 'build': report['build'], 'corpus_sha256': report['corpus_sha256'], 'harness_sha256': report['harness_sha256'], 'model': report['model'], 'configuration': report['configuration'], 'adapter_files': [{'file': Path(source['argument']).name, 'sha256': source['sha256']} for source in report['adapter_files']], 'repeats': report['repeats'], 'counts': report['counts'], 'arms': {name: {'runs': len(records), 'tool_calls': sum(r['tool_calls'] for r in records), 'output_bytes': sum(r['output_bytes'] for r in records), 'elapsed_ms': round(sum(r['elapsed_ms'] for r in records), 3), **aggregate_usage(records)} for name, records in arms.items()}, 'scenarios': [{'id': r['scenario'], 'arm': r['arm'], 'repeat': r['repeat'], 'order': r['order'], 'prompt_sha256': r['prompt_sha256'], 'status': r['score']['status'], **{k:r[k] for k in ('execution_status','answer_available','budget_compliance','validation_status','factual_quality') if k in r}, 'tool_calls': r['tool_calls'], 'output_bytes': r['output_bytes'], 'elapsed_ms': r['elapsed_ms'], 'usage': r['usage'], 'contexts': {name: {'snapshot_id': context['snapshot_id'], 'profile_sha256': context['profile_sha256'], 'inputs': [{'label': source['label'], 'sha256': source['sha256'], 'input_id': source['input_id']} for source in context['inputs']]} for name, context in r['contexts'].items()}} for r in report['records']], 'limitations': report['limitations']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(aggregate(json.loads(args.report.read_text(encoding='utf-8'))), indent=2) + '\n', encoding='utf-8')
