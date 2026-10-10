#!/usr/bin/env python3
"""Separate tool truth, supplied-fact interpretation and paired workflow checks."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import time
from collections import Counter
from agents import BASE_PROMPT, BUDGETS, contracts, external, nonsecret_configuration
from attempt import Allocation, Attempt, BudgetExceeded, BudgetUnknown, evaluate_attempt, aggregate_usage
from investigation import ROOT, Tools, address, contexts_for, digest, reference, require, score, timestamp
from schema import validate
from scripted import execute as legacy_execute, interpret
from unified import UnifiedTools, execute as unified_execute


class FactTools(Tools):
    contract = {'tools': [], 'scope': 'Verified source and operation facts are supplied directly. Reconstruction, profile and source tools are disabled.'}
    def _invoke(self, request): raise AssertionError('interpretation arm has no reconstruction tools')


def frozen_cases():
    manifest = json.loads((ROOT / 'evals/layer-cases.json').read_text())
    require(manifest['version'] == 1, 'unsupported frozen corpus')
    for label, sha in manifest['files'].items():
        require(hashlib.sha256((ROOT / label).read_bytes()).hexdigest() == sha, 'frozen corpus changed: ' + label)
    facts = json.loads((ROOT / 'evals/verified-facts.json').read_text())
    require(facts['version'] == 1, 'unsupported verified fact packet')
    return manifest, facts['cases']


def verified_packet(facts, contexts):
    observations = {}
    for group, packet in facts.items():
        def ref(coordinate, redacted=False):
            label, line, row = coordinate
            source = next(s for s in contexts[group]['inputs'] if s['label'] == label)
            result = reference(source, line, row)
            if redacted: result.update(row_path='[REDACTED PATH]', location_redacted=True)
            else: address(result, contexts[group])
            return result
        rows = [{'fields': row['fields'], 'ref': ref(row['source'], row.get('location_redacted', False))} for row in packet['rows']]
        operations = []
        for operation in packet['operations']:
            require(int((timestamp(operation['end_time']) - timestamp(operation['start_time'])).total_seconds() * 1000) == operation['duration'], 'independently authored packet arithmetic mismatch')
            operations.append({**operation, 'start': ref(operation['start']), 'end': ref(operation['end'])})
        observations[group] = rows, operations
    return observations


def timestamp_truth(artifact, truth, coordinate):
    def check(actual, expected):
        value, source = timestamp(actual), timestamp(expected)
        require(value == source and value.utcoffset() == source.utcoffset(), 'retained source timestamp or offset differs from literal truth')
    records = {coordinate(r['occurrence']['evidence_ref']):r for r in artifact['records']}
    for row in truth['rows']:
        if row['fields'] is not None:
            fields = row['fields']
            check(records[tuple(row['source'])]['timestamp'], fields.get('ts', fields.get('timestamp')))
    measurements = {tuple(coordinate(f['details']['boundaries'][phase]['occurrence']['evidence_ref']) for phase in ('start','end')):f for f in artifact['findings'] if f['kind'] == 'measurement'}
    for operation in truth['operations']:
        measurement = measurements[tuple(operation['start']),tuple(operation['end'])]
        for phase in ('start','end'):
            check(measurement['details']['boundaries'][phase]['timestamp'], operation[phase+'_time'])


def classification_truth(artifact, truth, coordinate, unmatched_starts=(), unsupported=False, unsuitable=False):
    records = {coordinate(r['occurrence']['evidence_ref']): r for r in artifact['records']}
    expected = {(tuple(o['start']), tuple(o['end'])): o for o in truth['operations']}
    observed = {}
    for membership in artifact['memberships']:
        if not membership['population_id'].endswith('-paired-lifecycles'): continue
        for member in membership['members']:
            sources = tuple(coordinate(o['evidence_ref']) for o in member['source_occurrences'])
            require(len(sources) == 2 and sources in expected, 'tool invented a lifecycle membership')
            require(sources not in observed, 'tool duplicated a lifecycle membership')
            identities = {i['field']: i['value'] for i in member['identity']}
            operation = expected[sources]
            require(identities == {'kind':'Request', 'name':operation['name'], 'correlation_id':operation['id'], 'scope':json.dumps([operation['scope']], separators=(',', ':'))}, 'tool lifecycle identity differs from literal truth')
            for phase, source in zip(('start', 'end'), sources):
                classification = records[source]['fields']['classification']
                require(classification['status'] == 'event', 'tool boundary classification unavailable')
                semantics = classification['semantics']
                require((semantics['kind'], semantics['name'], semantics['correlation_id'], semantics['scope'], semantics['phase']) == ('request', operation['name'], operation['id'], [operation['scope']], phase), 'tool boundary semantics differ from literal truth')
                require(semantics['outcome'] == (operation['outcome'] if phase == 'end' else None), 'tool outcome differs from literal truth')
            observed[sources] = member
    require(set(observed) == set(expected), 'tool omitted literal lifecycle memberships')
    missing = set()
    for finding in artifact['findings']:
        if finding['kind'] == 'observation' and finding['claim'] == 'A start has no observed end in this selected capture; this does not establish a hang.':
            missing.update(coordinate(o['evidence_ref']) for o in finding['details']['supporting_occurrences'])
    require(missing == set(map(tuple, unmatched_starts)), 'tool missing-end evidence differs from literal truth')
    for source in unmatched_starts:
        require(records[tuple(source)]['fields']['classification']['semantics']['phase'] == 'start', 'unmatched source is not a classified start')
    if unsupported:
        require(not records and all(s['semantic_coverage']['status'] == 'insufficient_evidence' for s in artifact['scopes']), 'unparsed input reported semantic support')
        require(all(p['entity'] == 'physical_records' for p in artifact['populations']), 'unparsed input invented lifecycle populations')
    if unsuitable:
        require(all(r['fields']['classification']['status'] == 'unclassified' for r in records.values()), 'unsuitable profile invented classified events')
        require(all(s['semantic_coverage']['status'] == 'insufficient_evidence' for s in artifact['scopes']), 'unsuitable profile reported semantic support')


def tool_truth(tools, public, facts):
    """Check direct Rust facts against literal truth, without the adapter/interpreter."""
    checks = []
    for group in public['groups']:
        report = tools.invoke({'group': group, 'tool': 'investigate'})['report']
        artifact = json.loads(tools.saved[group]['path'].read_bytes())
        truth = facts[group]
        parsed = len(truth['rows']) if any(r.get('location_redacted') for r in truth['rows']) else sum(r['fields'] is not None for r in truth['rows'])
        require(report['report_metadata']['evidence']['inputs'][0]['coverage']['parsed_entries'] == parsed, 'tool parsed count differs from literal truth')
        require(len(artifact['records']) == parsed, 'canonical occurrence count differs from literal truth')
        redacted = artifact['content'] == 'redacted'
        if not redacted:
            expected = {tuple(r['source']):r['fields']['level'] for r in truth['rows'] if r['fields'] is not None}
            observed = {address(r['occurrence']['evidence_ref'], tools.contexts[group]):r['fields']['level'] for r in artifact['records']}
            require(observed == expected, 'canonical severity/location differs from independently authored facts')
            measurements = [f for f in artifact['findings'] if f['kind'] == 'measurement']
            wanted = {(tuple(o['start']),tuple(o['end']),o['duration']) for o in truth['operations']}
            actual = {(address(f['details']['boundaries']['start']['occurrence']['evidence_ref'], tools.contexts[group]), address(f['details']['boundaries']['end']['occurrence']['evidence_ref'], tools.contexts[group]),f['details']['value']) for f in measurements}
            require(actual == wanted, 'source-backed timings/boundaries differ from independent truth')
            timestamp_truth(artifact, truth, lambda ref:address(ref, tools.contexts[group]))
            unmatched = [truth['rows'][0]['source']] if public['id'] in {'incomplete-capture','heldout-incomplete'} else []
            classification_truth(artifact, truth, lambda ref:address(ref, tools.contexts[group]), unmatched, unsupported=parsed == 0, unsuitable=public['id'] == 'unsuitable-profile')
        else:
            require(all(r['fields'] == {} and r['data_omitted'] for r in artifact['records']), 'redacted fields were reconstructed')
        require(all(s['upstream_completeness'] == 'unknown' for s in report['scopes']), 'tool invented capture completeness')
        checks.append({'group':group,'parsed_entries':parsed,'severity':'withheld' if redacted else 'checked','source_intervals':'withheld' if redacted else 'checked','upstream_completeness':'unknown'})
    return {'status':'PASS','checks':checks}


def metrics(tools):
    requests = [c['request'] for c in tools.calls]
    repetitions = Counter(digest(r) for r in requests)
    validation = [c for c in tools.calls if c['request'].get('command') == 'validate-profile' or c['request'].get('tool') == 'investigate']
    executions = [c.get('output',{}).get('report',{}).get('report_metadata',{}).get('evidence',{}).get('query',{}).get('execution',{}) for c in tools.calls if c['request'].get('tool') == 'investigate']
    reconstruction = any(r.get('tool') in {'analyzer','investigate'} for r in requests)
    return {'profile_resolution_status':'not_exercised_explicit_profile','profile_resolution_calls':0,
            'validation_adherence':{'support_checks_delivered':sum('output' in c for c in validation),'participant_attention':'unmeasured'},
            'repeated_queries':sum(n-1 for n in repetitions.values()),'custom_scripts':sum(r.get('tool') == 'interval' for r in requests),
            'analysis_command_invocations':sum(r.get('tool') in {'analyzer','investigate'} for r in requests),
            'tool_calls':len(tools.calls),'output_bytes':tools.output_bytes,
            'parse_passes':sum(e['parse_passes'] for e in executions) if executions and all('parse_passes' in e for e in executions) else None if reconstruction else 0,
            'correlation_passes':sum(e['correlation_passes'] for e in executions) if executions and all('correlation_passes' in e for e in executions) else None if reconstruction else 0}


def run(binary, repeats=2, adapter=None, model=None, config=None, allocated_budget_usd=None):
    require(repeats >= 2, 'layered comparisons require at least two repetitions')
    require(not adapter or (model and type(allocated_budget_usd) in {float,int} and math.isfinite(allocated_budget_usd) and allocated_budget_usd > 0), 'real-model runs require model identity and an explicitly allocated positive budget')
    nonsecret_configuration(config or {})
    binary = Path(binary).resolve(strict=True)
    manifest, packets = frozen_cases()
    initial_manifest, initial_packets = digest(manifest), digest(packets)
    paths = ['evals/'+name for name in ('layers.py','unified.py','attempt.py','agents.py','investigation.py','scripted.py','schema.py')]
    initial_harness = digest([[p,hashlib.sha256((ROOT/p).read_bytes()).hexdigest()] for p in paths])
    cap_started = time.monotonic()
    cap_bytes = subprocess.check_output([str(binary),'capabilities'], cwd=ROOT, timeout=30)
    cap_ms = (time.monotonic()-cap_started)*1000
    capabilities = json.loads(cap_bytes)
    require(capabilities['investigation_contracts']['command_available'] and capabilities['investigation_contracts']['artifact_retrieval_available'], 'unified binary required')
    records = []
    allocation = Allocation(allocated_budget_usd) if adapter else None
    for case in manifest['cases']:
        if not case['eligible']: continue
        scenario = case['scenario']
        contexts, preflight = contexts_for(binary, scenario)
        supplied = verified_packet(packets[case['id']],contexts)
        public = {k:scenario[k] for k in ('id','question','profile','groups','tasks')}
        public.update(contexts=contexts, vocabulary={'kinds':['observation','measurement','contrary_evidence','unknown'],'final':{'status':'map group to supported|insufficient_evidence|unsupported_input|budget_exhausted','findings':'typed group,subject,predicate,kind,value,refs; measurements require boundaries.start/end'}})
        plans = [('tool_correctness','tool-only',0,0)]
        for repeat in range(repeats):
            plans.append(('verified_facts_interpretation','verified-facts',repeat,0))
            plans += [('end_to_end',arm,repeat,order) for order,arm in enumerate(['legacy','unified'] if repeat%2 == 0 else ['unified','legacy'])]
        for layer,arm,repeat,order in plans:
            tools = UnifiedTools(binary,scenario,contexts,BUDGETS,capabilities['report_schemas']) if arm in {'unified','tool-only'} else FactTools(binary,scenario,arm,contexts,BUDGETS) if arm == 'verified-facts' else Tools(binary,scenario,'analyzer',contexts,BUDGETS)
            task = {**public, 'verified_facts':supplied} if arm == 'verified-facts' else public
            started = time.monotonic()
            preflight_ms = cap_ms + sum(p['elapsed_ms'] for p in preflight)
            tools.started -= preflight_ms/1000
            tools.output_bytes = len(cap_bytes) + sum(p['output_bytes'] for p in preflight) + len(json.dumps(task,ensure_ascii=False).encode())
            reserved_calls = 1 + len(preflight)
            tools.budgets = {**BUDGETS,'tool_calls':BUDGETS['tool_calls']-reserved_calls}
            attempt = Attempt()
            if arm == 'verified-facts':
                for group,(rows,operations) in supplied.items():
                    # Only the interpretation participant receives the verified packet.
                    for row in rows:
                        ref=row['ref']
                        if ref['location_redacted']:tools.revealed[group].add('__location_loss__')
                        else:tools.revealed[group].add(ref['reference_id'])
                    if rows and all(r['fields'] is None for r in rows) and not any(r['ref']['location_redacted'] for r in rows):tools.revealed[group].add('__unsupported__')
            def participate():
                if tools.output_bytes > tools.budgets['output_bytes']:raise BudgetExceeded('preflight output budget exhausted')
                if layer == 'tool_correctness':
                    attempt.receive({'final':tool_truth(tools,public,packets[case['id']])})
                elif adapter:
                    external(tools,task,adapter,model,config or {},attempt,allocation.remaining())
                else:
                    answer = interpret(public,supplied) if arm == 'verified-facts' else (unified_execute(tools,public) if arm == 'unified' else legacy_execute(tools,public))[0]
                    attempt.receive({'final':answer})
            def check_answer(answer,factual):
                if layer == 'tool_correctness':return []
                require(isinstance(answer,dict) and set(answer.get('status',{})) == set(contexts),'typed scopes differ')
                final = contracts(answer,contexts)
                for contract in final:validate(contract,capabilities['report_schemas']['investigation'])
                return final
            outcome = evaluate_attempt(attempt,participate,lambda answer:answer if layer == 'tool_correctness' else score(scenario,answer,contexts,tools.revealed),check_answer,tools.remaining)
            usage = attempt.usage('adapter_reported_unverified' if adapter and layer != 'tool_correctness' else 'unavailable_for_scripted_participant')
            if allocation: allocation.record(attempt)
            record = {'scenario':case['id'],'family':case['family'],'split':case['split'],'layer':layer,'arm':arm,'repeat':repeat,'order':order,'prompt_sha256':digest({'base':BASE_PROMPT,'task':task}),'budgets':BUDGETS,'elapsed_ms':round((time.monotonic()-started)*1000+preflight_ms,3),'common_preflight_elapsed_ms':round(preflight_ms,3),'usage':usage,'final':attempt.answer,'trace':tools.calls,**metrics(tools),**outcome}
            record['tool_calls'] += reserved_calls
            records.append(record)
            if isinstance(tools,UnifiedTools):tools.close()
    current, current_packets = frozen_cases()
    require(digest(current) == initial_manifest and digest(current_packets) == initial_packets,'frozen corpus changed during run')
    require(initial_harness == digest([[p,hashlib.sha256((ROOT/p).read_bytes()).hexdigest()] for p in paths]), 'harness changed during evaluation')
    return {'version':1,'kind':'optional_model_layered_evaluation' if adapter else 'scripted_layered_smoke','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'build':capabilities['build'],'corpus_sha256':initial_manifest,'harness_sha256':initial_harness,'model':model if adapter else None,'configuration':config or {},'allocated_budget_usd':allocated_budget_usd,'repeats':repeats,'exclusions':[{k:c[k] for k in ('id','family','split','exclusion_reason')} for c in manifest['cases'] if not c['eligible']], 'counts':dict(Counter(r['score']['status'] for r in records)),'records':records,'limitations':['Scripted repetitions check harness behavior; no real-model quality, variance, accuracy or cost improvement was measured.','Truth/rubrics remain outside participant messages; only interpretation receives verified source/operation facts. Trusted adapters share the filesystem and are not sandboxed.','Explicit profiles were supplied; automatic profile resolution and agent attention were not exercised. Validation evidence delivery is measured, not inferred attention.','Common capability/scope preflight is charged to every arm, including its analyzer call and runtime.','Output volume counts received subprocess stdout, including failed commands, and serialized local tool/fact responses. Tokens are never estimated from bytes.','Usage is incremental per response and adapter-reported unverified. Known partial totals survive failures; unknown spend stops subsequent paid calls.','Artifact/items use advertised schemas; retrieval envelopes use explicit broker hash/count/cursor invariants.','Unrestricted prose and causal reasoning require separate calibrated human review; that review and actual model runs were not performed.']}


def publish(report):
    layers = {}
    for name in sorted({r['layer'] for r in report['records']}):
        selected = [r for r in report['records'] if r['layer'] == name]
        layers[name] = {'counts':dict(Counter(r['score']['status'] for r in selected)),'arms':{arm:{'attempts':len(records),'tool_calls':sum(r['tool_calls'] for r in records),'output_bytes':sum(r['output_bytes'] for r in records),'elapsed_ms':round(sum(r['elapsed_ms'] for r in records),3),**aggregate_usage(records)} for arm in sorted({r['arm'] for r in selected}) if (records := [r for r in selected if r['arm'] == arm])}}
    keep = ('scenario','family','split','layer','arm','repeat','order','prompt_sha256','elapsed_ms','common_preflight_elapsed_ms','tool_calls','output_bytes','profile_resolution_status','profile_resolution_calls','validation_adherence','repeated_queries','custom_scripts','analysis_command_invocations','parse_passes','correlation_passes','usage','execution_status','answer_available','budget_compliance','validation_status')
    return {**{k:v for k,v in report.items() if k != 'records'},'layers':layers,'records':[{**{k:r[k] for k in keep},'factual_status':r['factual_quality']['status'],'status':r['score']['status'],'error_count':len(r['execution_errors'])} for r in report['records']]}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--report',type=Path,default=ROOT/'target/evals/layers.json')
    parser.add_argument('--publish',type=Path)
    parser.add_argument('--repeats',type=int,default=2)
    parser.add_argument('--adapter',nargs='+')
    parser.add_argument('--model')
    parser.add_argument('--configuration',type=Path)
    parser.add_argument('--allocated-budget-usd',type=float)
    args=parser.parse_args()
    try:
        config=json.loads(args.configuration.read_text()) if args.configuration else {}
        require(isinstance(config,dict),'configuration must be an object')
        result=run(args.binary,args.repeats,args.adapter,args.model,config,args.allocated_budget_usd)
        args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(result,indent=2)+'\n')
        if args.publish:args.publish.parent.mkdir(parents=True,exist_ok=True);args.publish.write_text(json.dumps(publish(result),indent=2)+'\n')
        print(result['counts'])
        for record in result['records']:
            if record['score']['status'] != 'PASS':print(record['scenario'],record['layer'],record['arm'],record['score'])
        return int(any(r['score']['status'] != 'PASS' for r in result['records']))
    except (AssertionError,OSError,ValueError,KeyError,subprocess.SubprocessError) as error:
        print(str(error),file=sys.stderr);return 2


if __name__ == '__main__':sys.exit(main())
