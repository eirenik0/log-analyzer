#!/usr/bin/env python3
"""Compile explicitly selected question mappings; never interpret question prose."""
import argparse
import copy
import json
import hashlib
import subprocess
import sys
from pathlib import Path

CHECKS = ['event_subjects', 'duration_endpoints_and_units', 'operation_types_and_count_semantics',
          'terminal_later_cached_contrary', 'summary_consistency', 'causal_support']


def load(path):
    with Path(path).open(encoding='utf-8') as stream:
        return json.load(stream)


def read_artifact(path, limit):
    if path.stat().st_size > limit:
        raise ValueError('Artifact exceeds --artifact-max-bytes (default: 64 MiB)')
    content, digest = bytearray(), hashlib.sha256()
    with path.open('rb') as stream:
        while chunk := stream.read(min(65536, limit - len(content) + 1)):
            if len(content) + len(chunk) > limit:
                raise ValueError('Artifact exceeds --artifact-max-bytes while reading')
            digest.update(chunk)
            content.extend(chunk)
    return json.loads(content), digest.hexdigest()


def required_ids(artifact):
    if 'question_checklist' in artifact:
        return artifact['question_checklist']['required_relationship']
    command = artifact.get('report_metadata', {}).get('evidence', {}).get('query', {}).get('command', {})
    return command.get('Investigate', {}).get('required_relationship', []) if isinstance(command, dict) else []


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
        subject = claim['subject']
        result.update(finding_id=finding['id'], pointer=claim['pointer'], start_operation=subject['start_operation'],
                      end_operation=subject['end_operation'])
    elif claim['kind'] == 'link':
        result.update(finding_id=claim['finding_id'], step_id=claim['step_id'])
        subject = claim['subject']
        for fi, finding in enumerate(artifact['findings']):
            if finding['id'] != claim['finding_id']:
                continue
            for si, step in enumerate(finding['details']['event_chain'].get('steps', [])):
                if step['id'] != claim['step_id']:
                    continue
                for ji, join in enumerate(step.get('join_samples', [])):
                    if join['source'] == subject['source'] and join['target'] == subject['target']:
                        result['pointer'] = f'/findings/{fi}/details/event_chain/steps/{si}/join_samples/{ji}'
                        return result
        raise ValueError('Link must select an exact retained source/target join')
    elif claim['kind'] == 'distinct_records':
        result['population'] = {'pointer': claim['pointer'], 'operation': claim.get('operation')}
    elif claim['kind'] == 'fact':
        result['field'] = {'pointer': claim['pointer'], 'equals': claim['equals']}
    else:
        raise ValueError('Unsupported question claim kind')
    return result


def candidates(draft, artifact):
    """Propose mechanically retained facts; never choose a question's subject."""
    draft = copy.deepcopy(draft)
    operations = {}
    for row in artifact.get('records', []):
        classification = row.get('fields', {}).get('classification', {})
        semantics = classification.get('semantics', {})
        kind, name = semantics.get('kind'), semantics.get('name')
        if (classification.get('status') != 'event' or kind not in ('command', 'request', 'event')
                or not isinstance(name, str) or not name):
            operations.clear()
            break
        operations.setdefault((kind, name), set()).add(json.dumps(row['occurrence'], sort_keys=True))
    for (kind, name), occurrences in sorted(operations.items()):
        cid = 'operation-' + hashlib.sha256(json.dumps([kind, name]).encode()).hexdigest()[:16]
        draft['claims'].append({'id': cid, 'kind': 'distinct_records', 'pointer': '/records',
                               'operation': {'kind': kind, 'name': name}, 'expected': len(occurrences)})
    if artifact.get('processing', {}).get('status') is not None:
        draft['claims'].append({'id': 'input-processing', 'kind': 'fact', 'pointer': '/processing/status',
                                'equals': artifact['processing']['status']})
    return draft


def prepare_plan(artifact):
    ids = required_ids(artifact)
    if not ids:
        raise ValueError('Register --required-relationship IDs when investigating before preparing a plan')
    return {'questions': [{'id': qid} for qid in ids], 'supporting_claim_ids': [], 'reviewed_checks': []}


def new_path(path):
    for index in range(101):
        candidate = path if not index else path.with_name(f'{path.stem}-{index}{path.suffix}')
        if not candidate.exists():
            return candidate
    raise ValueError('Choose a new output directory; too many existing outputs')


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
        if set(item) == {'id'}:
            raise ValueError(f'Question {qid}: independently select a claim and binding, or a typed anchored unknown')
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
            selectors = {'interval': {'start_operation', 'end_operation', 'pointer'}, 'link': {'step_id', 'source', 'target'},
                         'distinct_records': {'population'}, 'fact': {'field'}}
            if item['kind'] not in selectors or set(item) - ({'id', 'kind', 'finding_id', 'reason'} | selectors[item['kind']]):
                raise ValueError('Unknown mapping kind or fields')
            chain = chain_by_id[item['finding_id']]
            reason = item['reason']
            if not isinstance(reason, str) or not reason.strip() or len(reason) > 4096:
                raise ValueError('Unknown needs a nonempty bounded reason')
            spec = {'id': qid, 'kind': item['kind'], 'finding_id': item['finding_id']}
            if item['kind'] == 'interval':
                starting = item.get('start_operation', chain['root_event_name'])
                ending = item['end_operation']
                if not isinstance(starting, str) or not starting.strip() or not isinstance(ending, str) or not ending.strip():
                    raise ValueError('Unknown interval needs source-supported expected endpoint names')
                spec.update(start_operation=starting, end_operation=ending)
                if 'pointer' in item:
                    spec['pointer'] = item['pointer']
            elif item['kind'] == 'link':
                step = item['step_id']
                if not isinstance(step, str) or not step.strip():
                    raise ValueError('Unknown link needs the requested step_id')
                spec['step_id'] = step
                if 'source' in item or 'target' in item:
                    spec.update(source=copy.deepcopy(item['source']), target=copy.deepcopy(item['target']))
            elif item['kind'] == 'distinct_records':
                spec['population'] = copy.deepcopy(item['population'])
            elif 'field' in item:
                spec['field'] = copy.deepcopy(item['field'])
            requirements.append(spec)
            unresolved.append({'id': qid, 'reason': reason})
    if not ids or len(ids) > 100:
        raise ValueError('Select 1 to 100 question IDs')
    initial = required_ids(artifact)
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
        return {k: v for k, v in {'input_ordinal': occurrence.get('input_ordinal'),
                                  'line': ref.get('line'), 'row_path': ref.get('row_path')}.items()
                if v is not None}

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
            if key in subject:
                row[key] = address(subject[key])
        for key in ('expected_ms', 'expected'):
            if key in claim:
                row[key] = claim[key]
        options.append(row)
    initial = required_ids(artifact)
    return {'required_initial_ids': initial, 'required_review_checks': CHECKS,
            'event_candidates': [{'finding_id': fid, 'start_operation': c['root_event_name'],
                                 'root': address(c['root']), 'identity': c.get('identity'),
                                 'scope_fields': c.get('scope_fields'), 'context_identity': c.get('context_identity')}
                                for fid, c in anchors(artifact).items()],
            'supported_options': options,
            'limitation': 'Candidates are not independent truth. Select the intended subject before checksum-bound verification.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--draft', required=True)
    parser.add_argument('--artifact', required=True)
    parser.add_argument('--prepare-plan', type=Path, help='Create an unselected checklist plan and return the verification command')
    parser.add_argument('--finding-id', help='Target one retained finding when creating a new draft')
    parser.add_argument('--plan', help='JSON with questions selecting {id,claim_id,binding} (population instead of binding for counts) or declaring {id,kind,finding_id,reason} plus end_operation, step_id, population or optional field; reviewed_checks and optional supporting_claim_ids')
    parser.add_argument('--output', help='New assertions file; never overwrites')
    parser.add_argument('--analyzer', type=Path, help='Analyzer executable; required for plans or creating a missing draft')
    parser.add_argument('--artifact-max-bytes', type=int, help='Explicit artifact read allowance forwarded to drafting and verification (native default: 64 MiB)')
    parser.add_argument('--verification-output', type=Path, help='New retained verification report; required for plans')
    args = parser.parse_args()
    try:
        if args.artifact_max_bytes is not None and not 0 < args.artifact_max_bytes <= 2**64 - 1:
            raise ValueError('--artifact-max-bytes must be a positive u64 byte limit')
        allowance = [] if args.artifact_max_bytes is None else ['--artifact-max-bytes', str(args.artifact_max_bytes)]
        if args.prepare_plan and (args.plan or args.output or not args.analyzer):
            raise ValueError('--prepare-plan needs --analyzer and cannot be combined with --plan/--output')
        if args.prepare_plan:
            template_path = args.prepare_plan.resolve()
            if template_path.exists() or template_path in {Path(args.artifact).resolve(), Path(args.draft).resolve()}:
                raise ValueError('Plan template must be a new path distinct from artifact and draft')
        if bool(args.plan) != bool(args.output):
            raise ValueError('--plan and --output must be supplied together')
        artifact_path = Path(args.artifact).resolve()
        analyzer = args.analyzer.resolve() if args.analyzer else None
        if args.plan and (analyzer is None or args.verification_output is None):
            raise ValueError('Plans require --analyzer and --verification-output; compilation must be followed by retained verification')
        if args.plan:
            claims_path, verification_path, draft_path = Path(args.output).resolve(), args.verification_output.resolve(), Path(args.draft).resolve()
            if len({claims_path, verification_path, draft_path}) != 3:
                raise ValueError('Claims, verification and draft paths must be distinct')
            if claims_path.exists():
                raise ValueError('Claims output must be a new path')
        if args.plan and args.verification_output.exists():
            raise ValueError('Verification report must be a new path; do not reuse prior proof')
        artifact, sha = read_artifact(artifact_path, args.artifact_max_bytes or 67108864)
        template = prepare_plan(artifact) if args.prepare_plan else None
        plan = load(args.plan) if args.plan else None
        unknown_only = plan is not None and not plan.get('supporting_claim_ids') and all('claim_id' not in q for q in plan['questions'])
        if not Path(args.draft).exists() and not unknown_only and artifact.get('content') != 'redacted':
            if analyzer is None:
                raise ValueError('A missing draft requires --analyzer to create it from the retained snapshot')
            subprocess.run([str(analyzer), 'evidence', str(artifact_path), '--expected-sha256', sha,
                            '--draft-claims', '--output', args.draft] + allowance +
                           ([] if args.finding_id is None else ['--id', args.finding_id]), check=True,
                           stdout=subprocess.DEVNULL)
        draft = load(args.draft) if Path(args.draft).exists() else {'claims': []}
        draft = candidates(draft, artifact)
        if not args.plan:
            view = inventory(draft, artifact)
            command = None
            if args.prepare_plan:
                with template_path.open('x', encoding='utf-8') as stream:
                    json.dump(template, stream, ensure_ascii=False, indent=2)
                    stream.write('\n')
                command = [sys.executable, '-I', str(Path(__file__).resolve()), '--artifact', str(artifact_path),
                           '--draft', str(Path(args.draft).resolve()), '--plan', str(template_path),
                           '--output', str(new_path(template_path.with_name('claims.json'))),
                           '--analyzer', str(analyzer), '--verification-output',
                           str(new_path(template_path.with_name('verification.json')))] + allowance
            view = {'action_summary': {'required_question_count': len(required_ids(artifact)),
                     'next_step': 'independently_select_subjects_and_review_then_verify', 'next_command': command,
                     'plan_path': str(template_path) if args.prepare_plan else None}, **view}
            print(json.dumps(view, ensure_ascii=False, separators=(',', ':')))
            return
        result = compile_plan(draft, artifact, plan)
        with Path(args.output).open('x', encoding='utf-8') as stream:
            json.dump(result, stream, ensure_ascii=False, indent=2)
            stream.write('\n')
        executed = subprocess.run([str(analyzer), 'evidence', str(artifact_path),
                                   '--expected-sha256', sha, '--claims', args.output,
                                   '--output', str(args.verification_output)] + allowance,
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
