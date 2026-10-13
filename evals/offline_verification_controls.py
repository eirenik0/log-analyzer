#!/usr/bin/env python3
"""Public synthetic controls for the helper and acceptance gate; no model calls."""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from native_workflow import fixture, input_content, profile
from verification_required import QUESTION_IDS, acceptance_gate, coverage


def control_packet(directory, binary, repository, held_out=False, recognition_gap=False):
    directory.mkdir(parents=True, exist_ok=True)
    capture = directory / 'capture.jsonl'
    capture.write_text(input_content('held-out' if held_out else 'development'), encoding='utf-8')
    selected = directory / 'profile.toml'
    text = profile().replace('equals = "capture_end"', 'equals = "result"') if held_out and not recognition_gap else profile()
    selected.write_text(text, encoding='utf-8')
    artifact = directory / 'evidence.json'
    argv = [str(binary), '--config', str(selected), 'investigate',
            '--profiles-dir', str(repository / 'config/profiles'), str(capture), '--artifact', str(artifact), '--summary', '--question', 'Investigate the public incident']
    for qid in QUESTION_IDS: argv += ['--required-relationship', qid]
    subprocess.run(argv, check=True, stdout=subprocess.DEVNULL)
    helper = repository / '.agents/skills/analyze-logs/scripts/map_questions.py'
    spec = importlib.util.spec_from_file_location('offline_question_mapping', helper)
    mapping = importlib.util.module_from_spec(spec); spec.loader.exec_module(mapping)
    draft_path, plan_path = directory / 'draft.json', directory / 'plan.json'
    view = json.loads(subprocess.check_output([sys.executable, '-I', str(helper), '--artifact', str(artifact),
            '--draft', str(draft_path), '--analyzer', str(binary), '--prepare-plan', str(plan_path)]))
    blank = json.loads(plan_path.read_text(encoding='utf-8'))
    assert blank['questions'] == [{'id': qid} for qid in QUESTION_IDS]
    assert blank['reviewed_checks'] == []
    retained = json.loads(artifact.read_text(encoding='utf-8'))
    draft = mapping.candidates(json.loads(draft_path.read_text(encoding='utf-8')), retained)
    _, expected = fixture(held_out)
    requested = {k: (expected[k]['subject'], expected[k]['scope']) for k in ['capture', 'other', 'close']}
    fields = {r['occurrence']['evidence_ref']['line']: r['fields']['structured_fields'] for r in retained['records']}
    roots = {k: next(f for f in retained['findings'] if f.get('details', {}).get('event_chain')
             and fields[f['details']['event_chain']['root']['evidence_ref']['line']]['operation_name'] == requested[k][0]
             and fields[f['details']['event_chain']['root']['evidence_ref']['line']]['session_id'] == requested[k][1]) for k in requested}
    def choose(qid, claim):
        requirement = mapping.requirement(claim, retained)
        key = 'population' if claim['kind'] == 'distinct_records' else 'binding'
        binding = requirement['population'] if key == 'population' else {k: v for k, v in requirement.items() if k != 'kind'}
        return {'id': qid, 'claim_id': claim['id'], key: binding}
    questions = []
    for key in ['capture', 'close']:
        root = roots[key]['details']['event_chain']['root']
        ending = 'close_end' if key == 'close' else ('result' if held_out else 'capture_end')
        if key == 'capture' and recognition_gap:
            questions.append({'id': key, 'kind': 'interval', 'finding_id': roots[key]['id'],
                              'start_operation': requested[key][0], 'end_operation': 'capture completed',
                              'reason': 'No ending matched the current starting profile.'})
            continue
        selected_claim = next(c for c in draft['claims'] if c['kind'] == 'interval' and c['subject']['source'] == root
                              and fields[c['subject']['target']['evidence_ref']['line']]['event_type'] == ending)
        questions.append(choose(key, selected_claim))
    questions.append({'id': 'other', 'kind': 'interval', 'finding_id': roots['other']['id'],
                      'start_operation': requested['other'][0], 'end_operation': 'capture completed',
                      'reason': 'No supported capture ending for this scoped subject.'})
    for qid, name in [('send_count', 'send'), ('response_count', 'response')]:
        questions.append(choose(qid, next(c for c in draft['claims'] if c['kind'] == 'distinct_records'
                                          and c.get('operation') == {'kind': 'event', 'name': name})))
    questions.append({'id': 'cause', 'kind': 'fact', 'finding_id': roots['other']['id'],
                      'reason': 'No retained observations establish a causal mechanism.'})
    questions.append(choose('coverage', next(c for c in draft['claims'] if c['id'] == 'input-processing')))
    supporting = [c['id'] for c in draft['claims'] if c['kind'] == 'link' and c['subject']['root'] == roots['other']['details']['event_chain']['root']
                  and fields[c['subject']['target']['evidence_ref']['line']]['event_type'] in ('cached_failure', 'poll_response')]
    plan = {'questions': questions, 'supporting_claim_ids': supporting, 'reviewed_checks': mapping.CHECKS}
    plan_path.write_text(json.dumps(plan), encoding='utf-8')
    claims, report = directory / 'claims.json', directory / 'verification.json'
    subprocess.run([sys.executable, '-I', str(helper), '--artifact', str(artifact), '--draft', str(draft_path),
                    '--plan', str(plan_path), '--output', str(claims), '--analyzer', str(binary),
                    '--verification-output', str(report)], check=True, stdout=subprocess.DEVNULL)
    answer = copy.deepcopy(expected)
    answer.update(explanation='A cached failed result and later response were observed; they do not establish a cause.',
                  summary={'completed_subjects': [expected['capture']['subject'], 'close'],
                           'unresolved_subjects': [expected['other']['subject']], 'send_count': 1, 'response_count': 1},
                  verification={'artifact': str(artifact), 'claims': str(claims), 'report': str(report)},
                  claim_ids={**{qid: [qid] for qid in QUESTION_IDS}, 'explanation': supporting + ['cause']},
                  coverage=coverage(retained))
    if recognition_gap:
        answer['capture'].update(completion='unknown', elapsed_ms=None)
        answer['summary']['completed_subjects'] = ['close']
        answer['summary']['unresolved_subjects'] = [expected['capture']['subject'], expected['other']['subject']]
    return answer, capture, requested, view


def controls(binary, repository):
    results = []
    with tempfile.TemporaryDirectory(prefix='verification-controls-') as temporary:
        for held_out in [False, True]:
            directory = Path(temporary).resolve() / ('held-out' if held_out else 'development')
            answer, capture, requested, view = control_packet(directory, binary, repository, held_out)
            artifact = Path(answer['verification']['artifact']); original = artifact.read_bytes()
            cases = [('positive', answer, True)]
            for name, field, value in [('wrong-subject', 'subject', 'another operation'),
                                       ('wrong-scope', 'scope', 'another session'), ('wrong-duration', 'elapsed_ms', 49)]:
                wrong = copy.deepcopy(answer); wrong['capture'][field] = value; cases.append((name, wrong, False))
            wrong = copy.deepcopy(answer); wrong['send_count'] = 2; cases.append(('wrong-count', wrong, False))
            wrong = copy.deepcopy(answer); wrong['send_count'] = True; cases.append(('invalid-count-format', wrong, False))
            wrong = copy.deepcopy(answer); wrong['coverage']['parsed_entries'] = 0; cases.append(('wrong-coverage', wrong, False))
            wrong = copy.deepcopy(answer); wrong['claim_ids']['capture'] = ['close']; cases.append(('wrong-claim-id', wrong, False))
            wrong = copy.deepcopy(answer); wrong['other']['completion'] = True; cases.append(('unknown-as-complete', wrong, False))
            wrong = copy.deepcopy(answer); wrong['verification']['report'] = str(directory / 'missing.json'); cases.append(('missing-proof', wrong, False))
            for index, (name, proposed, accepted) in enumerate(cases):
                result = acceptance_gate(proposed, directory, binary, capture, requested, index, 10, 30)
                assert result['accepted'] == accepted, (name, result)
                results.append({'case': directory.name, 'control': name, 'status': 'passed', 'accepted': result['accepted']})
            artifact.write_bytes(original + b'\n')
            stale = acceptance_gate(answer, directory, binary, capture, requested, 20, 10, 30)
            assert not stale['accepted'] and any('stale' in issue for issue in stale['issues'])
            results.append({'case': directory.name, 'control': 'stale-proof', 'status': 'passed', 'accepted': False})
            artifact.write_bytes(original)
            claims = Path(answer['verification']['claims']); original_claims = claims.read_bytes()
            altered = json.loads(original_claims)
            next(c for c in altered['claims'] if c['id'] == 'capture')['expected_ms'] = 49
            claims.write_text(json.dumps(altered), encoding='utf-8')
            contradicted = acceptance_gate(answer, directory, binary, capture, requested, 21, 10, 30)
            assert not contradicted['accepted']
            results.append({'case': directory.name, 'control': 'contradicted-claims', 'status': 'passed', 'accepted': False})
            claims.write_bytes(original_claims)
            assert acceptance_gate(answer, directory, binary, capture, requested, 22, 10, 30)['accepted']
            forged = json.loads(original)
            spec = next(r for r in altered['requirements'] if r['id'] == 'capture')
            interval = forged
            for key in spec['pointer'].split('/')[1:]:
                interval = interval[int(key)] if isinstance(interval, list) else interval[key]
            interval.update(value=49, end_timestamp='2026-01-01T00:00:00.049Z')
            artifact.write_text(json.dumps(forged), encoding='utf-8')
            claims.write_text(json.dumps(altered), encoding='utf-8')
            proof = directory / 'altered-timestamp-proof.json'
            subprocess.run([str(binary), 'evidence', str(artifact), '--expected-sha256',
                            hashlib.sha256(artifact.read_bytes()).hexdigest(), '--claims', str(claims),
                            '--output', str(proof)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
            proposed = copy.deepcopy(answer); proposed['verification']['report'] = str(proof)
            proposed['capture']['elapsed_ms'] = 49
            result = acceptance_gate(proposed, directory, binary, capture, requested, 23, 10, 30)
            assert not result['accepted']
            results.append({'case': directory.name, 'control': 'altered-retained-timestamps', 'status': 'passed', 'accepted': False})
            artifact.write_bytes(original); claims.write_bytes(original_claims)
            forged = json.loads(original)
            forged['report_metadata']['evidence']['inputs'][0]['coverage']['parsed_entries'] = 0
            artifact.write_text(json.dumps(forged), encoding='utf-8')
            proof = directory / 'altered-coverage-proof.json'
            subprocess.run([str(binary), 'evidence', str(artifact), '--expected-sha256',
                            hashlib.sha256(artifact.read_bytes()).hexdigest(), '--claims', str(claims),
                            '--output', str(proof)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
            proposed = copy.deepcopy(answer); proposed['verification']['report'] = str(proof)
            proposed['coverage'] = coverage(forged)
            result = acceptance_gate(proposed, directory, binary, capture, requested, 24, 10, 30)
            assert not result['accepted'], result
            results.append({'case': directory.name, 'control': 'altered-retained-coverage', 'status': 'passed', 'accepted': False})
            artifact.write_bytes(original)

        directory = Path(temporary).resolve() / 'supported-recognition-gap'
        proposed, capture, requested, _ = control_packet(directory, binary, repository, held_out=True, recognition_gap=True)
        saved = json.loads(Path(proposed['verification']['report']).read_text(encoding='utf-8'))
        assert saved['artifact_retrieval']['claim_verification']['status'] == 'verified_with_unknowns'
        result = acceptance_gate(proposed, directory, binary, capture, requested, 0, 10, 30)
        assert not result['accepted'] and any('source-supported ending' in issue for issue in result['issues'])
        results.append({'case': 'held-out', 'control': 'supported-recognition-gap', 'status': 'passed', 'accepted': False})
    return {'kind': 'offline_verification_required_controls', 'model_calls': 0, 'controls': results,
            'limitations': ['Scripted controls validate mechanics, not model compliance or prose-review accuracy.']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--analyzer', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = controls(args.analyzer.resolve(), Path(__file__).resolve().parents[1])
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump(result, stream, indent=2); stream.write('\n')
    print(json.dumps({'status': 'passed', 'controls': len(result['controls']), 'model_calls': 0}))
