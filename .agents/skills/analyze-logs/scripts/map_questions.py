#!/usr/bin/env python3
"""Compile explicitly selected question mappings; never interpret question prose."""
import argparse
import copy
import json
import hashlib
import subprocess
from pathlib import Path

CHECKS = ['event_subjects', 'duration_endpoints_and_units', 'operation_types_and_count_semantics',
          'terminal_later_cached_contrary', 'summary_consistency', 'causal_support']


def load(path):
    with Path(path).open(encoding='utf-8') as stream:
        return json.load(stream)


def anchors(artifact):
    return {f['id']: f['details']['event_chain'] for f in artifact.get('findings', [])
            if isinstance(f.get('details', {}).get('event_chain'), dict)
            and f['details']['event_chain'].get('root_event_name') is not None}


def requirement(claim, artifact):
    result = {'kind': claim['kind']}
    if claim['kind'] == 'interval':
        parts = claim.get('pointer', '').split('/')
        if len(parts) < 3 or parts[1] != 'findings' or not parts[2].isdigit():
            raise ValueError('Interval must use the draft retained finding pointer')
        finding = artifact['findings'][int(parts[2])]
        chain = finding['details']['event_chain']
        subject = claim['subject']
        if subject['source'] != chain['root']:
            raise ValueError('A question interval must start at its retained scoped root')
        result.update(finding_id=finding['id'], start_operation=subject['start_operation'],
                      end_operation=subject['end_operation'])
    elif claim['kind'] == 'link':
        result.update(finding_id=claim['finding_id'], step_id=claim['step_id'])
    elif claim['kind'] == 'distinct_records':
        result['population'] = {'pointer': claim['pointer'], 'operation': claim.get('operation')}
    elif claim['kind'] == 'fact':
        result['field'] = {'pointer': claim['pointer'], 'equals': claim['equals']}
    else:
        raise ValueError('Unsupported question claim kind')
    return result


def compile_plan(draft, artifact, plan):
    claims_by_id = {c['id']: c for c in draft['claims']}
    if len(claims_by_id) != len(draft['claims']):
        raise ValueError('Duplicate draft claim ID')
    chain_by_id = anchors(artifact)
    claims, requirements, unresolved, ids = [], [], [], []
    used_claims = set()
    for item in plan['questions']:
        qid = item['id']
        if not isinstance(qid, str) or not qid.strip() or qid in ids:
            raise ValueError('Question IDs must be unique nonempty strings')
        ids.append(qid)
        if 'claim_id' in item:
            if item['claim_id'] in used_claims:
                raise ValueError('A supported question selects one distinct draft claim ID')
            claim = copy.deepcopy(claims_by_id[item['claim_id']])
            used_claims.add(item['claim_id'])
            spec = requirement(claim, artifact)
            key = 'population' if claim['kind'] == 'distinct_records' else 'binding'
            expected = spec['population'] if key == 'population' else {k: v for k, v in spec.items() if k != 'kind'}
            if set(item) != {'id', 'claim_id', key} or item[key] != expected:
                raise ValueError(f'Question needs an explicit matching {key} independent of its selected claim')
            requirements.append({'id': qid, **spec})
            claim['id'] = qid
            claims.append(claim)
        else:
            selectors = {'interval': {'end_operation'}, 'link': {'step_id'},
                         'distinct_records': {'population'}, 'fact': {'field'}}
            if item['kind'] not in selectors or set(item) - ({'id', 'kind', 'finding_id', 'reason'} | selectors[item['kind']]):
                raise ValueError('Unknown mapping kind or fields')
            chain = chain_by_id[item['finding_id']]
            reason = item['reason']
            if not isinstance(reason, str) or not reason.strip() or len(reason.encode('utf-8')) > 4096:
                raise ValueError('Unknown needs a nonempty bounded reason')
            spec = {'id': qid, 'kind': item['kind'], 'finding_id': item['finding_id']}
            if item['kind'] == 'interval':
                ending = item['end_operation']
                if not isinstance(ending, str) or not ending.strip():
                    raise ValueError('Unknown interval needs the source-supported expected end name')
                spec.update(start_operation=chain['root_event_name'], end_operation=ending)
            elif item['kind'] == 'link':
                step = item['step_id']
                if not isinstance(step, str) or not step.strip():
                    raise ValueError('Unknown link needs the requested step_id')
                spec['step_id'] = step
            elif item['kind'] == 'distinct_records':
                spec['population'] = copy.deepcopy(item['population'])
            elif 'field' in item:
                spec['field'] = copy.deepcopy(item['field'])
            requirements.append(spec)
            unresolved.append({'id': qid, 'reason': reason})
    if not ids or len(ids) > 100:
        raise ValueError('Select 1 to 100 question IDs')
    initial = artifact.get('report_metadata', {}).get('evidence', {}).get('query', {}).get('command', {}).get('Investigate', {}).get('required_relationship', [])
    if not set(initial).issubset(ids):
        raise ValueError('Plan cannot omit an initial required relationship ID')
    for cid in plan.get('supporting_claim_ids', []):
        if cid in used_claims or cid in ids:
            raise ValueError('Supporting claim must have its own distinct ID')
        used_claims.add(cid)
        claims.append(copy.deepcopy(claims_by_id[cid]))
    if len(claims) > 100:
        raise ValueError('At most 100 claims')
    checked = plan.get('reviewed_checks', [])
    if len(checked) != len(set(checked)) or set(checked) != set(CHECKS):
        raise ValueError('After inspection, explicitly list all six reviewed_checks; do not infer review')
    selected = [c['id'] for c in claims]
    return {'version': 1, 'questions': draft.get('questions', []), 'required_claim_ids': ids,
            'claims': claims, 'requirements': requirements, 'unresolved': unresolved,
            'final_review': {'finding_claim_ids': selected, 'summary_claim_ids': selected,
                             'unresolved_question_ids': [u['id'] for u in unresolved], 'checks': checked}}


def inventory(draft, artifact):
    def address(occurrence):
        ref = occurrence.get('evidence_ref', {})
        return {'input_ordinal': occurrence.get('input_ordinal'),
                'line': ref.get('line'), 'row_path': ref.get('row_path')}

    options = []
    for claim in draft['claims']:
        try:
            spec = requirement(claim, artifact)
        except (ValueError, KeyError, IndexError):
            continue
        row = {'claim_id': claim['id'], **spec}
        parts = claim.get('pointer', '').split('/')
        if len(parts) > 2 and parts[1] == 'findings' and parts[2].isdigit():
            row['finding_id'] = artifact['findings'][int(parts[2])]['id']
        subject = claim.get('subject', {})
        for key in ('source', 'target'):
            if key in subject and not (claim['kind'] == 'interval' and key == 'source'):
                row[key] = address(subject[key])
        for key in ('expected_ms', 'expected'):
            if key in claim:
                row[key] = claim[key]
        options.append(row)
    initial = artifact.get('report_metadata', {}).get('evidence', {}).get('query', {}).get('command', {}).get('Investigate', {}).get('required_relationship', [])
    return {'required_initial_ids': initial, 'required_review_checks': CHECKS,
            'unknown_anchors': [{'finding_id': fid, 'start_operation': c['root_event_name'],
                                 'root': address(c['root']), 'identity': c.get('identity')}
                                for fid, c in anchors(artifact).items()],
            'supported_options': options,
            'limitation': 'Compact selectors for an explicit plan, not verified truth. Full snapshot/subject addresses remain in the draft; compilation runs checksum-bound verification.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--draft', required=True)
    parser.add_argument('--artifact', required=True)
    parser.add_argument('--plan', help='JSON with questions selecting {id,claim_id,binding} (population instead of binding for counts) or declaring {id,kind,finding_id,reason} plus end_operation, step_id, population or optional field; reviewed_checks and optional supporting_claim_ids')
    parser.add_argument('--output', help='New assertions file; never overwrites')
    parser.add_argument('--analyzer', type=Path, help='Analyzer executable; required for plans or creating a missing draft')
    parser.add_argument('--verification-output', type=Path, help='New retained verification report; required for plans')
    args = parser.parse_args()
    try:
        artifact_path = Path(args.artifact).resolve()
        sha = hashlib.sha256(artifact_path.read_bytes()).hexdigest()
        analyzer = args.analyzer.resolve() if args.analyzer else None
        if args.plan and (analyzer is None or args.verification_output is None):
            raise ValueError('Plans require --analyzer and --verification-output; compilation must be followed by retained verification')
        if args.plan and args.verification_output.exists():
            raise ValueError('Verification report must be a new path; do not reuse prior proof')
        plan = load(args.plan) if args.plan else None
        unknown_only = plan is not None and not plan.get('supporting_claim_ids') and all('claim_id' not in q for q in plan['questions'])
        if not Path(args.draft).exists() and not unknown_only:
            if analyzer is None:
                raise ValueError('A missing draft requires --analyzer to create it from the retained snapshot')
            subprocess.run([str(analyzer), 'evidence', str(artifact_path), '--expected-sha256', sha,
                            '--draft-claims', '--output', args.draft], check=True,
                           stdout=subprocess.DEVNULL)
        draft = load(args.draft) if Path(args.draft).exists() else {'claims': []}
        artifact = load(artifact_path)
        if bool(args.plan) != bool(args.output):
            raise ValueError('--plan and --output must be supplied together')
        if not args.plan:
            print(json.dumps(inventory(draft, artifact), ensure_ascii=False, separators=(',', ':')))
            return
        result = compile_plan(draft, artifact, plan)
        with Path(args.output).open('x', encoding='utf-8') as stream:
            json.dump(result, stream, ensure_ascii=False, indent=2)
            stream.write('\n')
        executed = subprocess.run([str(analyzer), 'evidence', str(artifact_path),
                                   '--expected-sha256', sha, '--claims', args.output,
                                   '--output', str(args.verification_output)],
                                  stdout=subprocess.DEVNULL, check=False)
        if not args.verification_output.exists():
            raise ValueError('Verifier did not save a report; inspect its error and correct the plan')
        cv = load(args.verification_output).get('artifact_retrieval', {}).get('claim_verification', {})
        checks = []
        for c in cv.get('checks', []):
            observed = c.get('observed') or {}
            brief = {k: c.get(k) for k in ('id', 'status', 'assurance', 'reason')}
            if isinstance(observed, dict):
                brief['observed'] = {k: observed[k] for k in ('value', 'unit', 'start_operation', 'end_operation', 'source', 'target') if k in observed}
            else:
                brief['observed'] = observed
            if c.get('status') != 'verified':
                brief['expected'] = c.get('expected')
            checks.append(brief)
        print(json.dumps({'status': cv.get('status', 'unavailable'),
                          'verification_report': str(args.verification_output),
                          'artifact_sha256': sha, 'claims_path': args.output,
                          'requirement_results': cv.get('requirement_results', []),
                          'unresolved': cv.get('unresolved', []),
                          'final_review': cv.get('final_review'), 'checks': checks}, ensure_ascii=False))
        if executed.returncode or cv.get('status') not in ('verified', 'verified_with_unknowns'):
            parser.exit(1, 'Mapping verification failed; correct the plan and use new output paths\n')
    except (ValueError, KeyError, IndexError, TypeError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'Mapping error: {error}\n')


if __name__ == '__main__':
    main()
