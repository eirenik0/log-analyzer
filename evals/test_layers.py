import copy
import json
import unittest
from pathlib import Path
from attempt import Attempt, evaluate_attempt
from layers import timestamp_truth, classification_truth, frozen_cases, verified_packet
from schema import InvalidValue, validate
from unified import UnifiedTools


class LayeredHarnessTests(unittest.TestCase):
    def test_frozen_cases_include_visible_exclusions_and_heldout_variants(self):
        manifest, packets = frozen_cases()
        self.assertEqual(len(manifest['cases']), 17)
        self.assertEqual(sum(c['eligible'] for c in manifest['cases']), 14)
        self.assertEqual(sum(c['split'] == 'heldout' for c in manifest['cases']), 4)
        self.assertTrue(all(c['exclusion_reason'] for c in manifest['cases'] if not c['eligible']))
        for packet in packets.values():
            self.assertNotIn('expected', packet)
            self.assertNotIn('status', packet)
            for group in packet.values():
                self.assertEqual(set(group), {'rows','operations'})

    def test_null_received_answer_is_available_but_fails_validation(self):
        attempt = Attempt()
        def check(answer, factual): raise ValueError('invalid answer')
        result = evaluate_attempt(attempt,lambda:attempt.receive({'final':None}),lambda answer:{'status':'FAIL'},check,lambda:None)
        self.assertTrue(result['answer_available'])
        self.assertEqual(result['validation_status'],'failed')
        self.assertEqual(result['factual_quality']['status'],'FAIL')

    def test_compositions_conditionals_and_bounds_are_enforced(self):
        schema = {'allOf':[{'anyOf':[{'type':'integer','minimum':2,'maximum':4},{'type':'string','minLength':2}]}], 'if':{'type':'string'},'then':{'pattern':'^ok'},'else':{'const':3}}
        for value in (3,'okay'):validate(value,schema)
        for value in (1,2,4,5,'o','bad',True):
            with self.assertRaises(InvalidValue):validate(value,schema)
        with self.assertRaises(InvalidValue):validate([1,2],{'type':'array','maxItems':1})
        with self.assertRaises(InvalidValue):validate({'x':'wrong'},{'type':'object','additionalProperties':{'type':'integer'}})
        with self.assertRaises(ValueError):validate({}, {'newAssertion':True})
        with self.assertRaises(InvalidValue):validate('2026-01-01T00:00:00',{'type':'string','format':'date-time'})

    def test_invalid_pages_cannot_receive_citation_credit(self):
        tools = object.__new__(UnifiedTools)
        tools.pages={}
        tools.schemas={'evidence_artifact':{'$defs':{'retained_record':{'type':'object','required':['occurrence']}}}}
        page={'artifact_sha256':'a'*64,'binding_sha256':'b'*64,'collection':'/records','contract_version':1,'correlation_passes':0,'displayed':1,'id':None,'items':[{'occurrence':{}}],'next_cursor':None,'parse_passes':0,'prior':0,'remaining':0,'source_verification':{'facts':'retained_snapshot','reason':'not read','status':'not_requested'},'status':'complete','total':1}
        request={'group':'run','tool':'evidence','collection':'/records'}
        tools.saved = {'run': {'artifact': {'records': copy.deepcopy(page['items'])}}}
        tools.validate_page(page,request)
        for name,value in [('displayed',2),('prior',1),('next_cursor','bad'),('collection','/findings'),('contract_version',2),('items',[{}]),('items',[{'occurrence':{'wrong':True}}]),('binding_sha256','bad'),('remaining',1),('total',True)]:
            altered=copy.deepcopy(page);altered[name]=value
            with self.assertRaises(AssertionError):tools.validate_page(altered,request)
        with self.assertRaises(AssertionError):tools.validate_page(page,{**request,'cursor':'v1:'+('b'*64)+':0'})

    def test_well_formed_page_with_changed_total_is_rejected(self):
        tools = object.__new__(UnifiedTools)
        tools.pages = {}
        tools.schemas = {'evidence_artifact': {'$defs': {'retained_record': {'type': 'object'}}}}
        tools.saved = {'run': {'artifact': {'records': [{'index': 0}, {'index': 1}]}}}
        page = {'artifact_sha256': 'a'*64, 'binding_sha256': 'b'*64, 'collection': '/records', 'contract_version': 1, 'correlation_passes': 0, 'displayed': 1, 'id': None, 'items': [{'index': 0}], 'next_cursor': 'v1:'+('b'*64)+':1', 'parse_passes': 0, 'prior': 0, 'remaining': 1, 'source_verification': {'facts': 'retained_snapshot', 'reason': 'not read', 'status': 'not_requested'}, 'status': 'page', 'total': 2}
        request = {'group': 'run', 'tool': 'evidence', 'collection': '/records'}
        tools.validate_page(page, request)
        altered = {**page, 'prior': 1, 'items': [{'index': 1}], 'total': 3, 'next_cursor': 'v1:'+('b'*64)+':2'}
        with self.assertRaisesRegex(AssertionError, 'total differs'):
            tools.validate_page(altered, {**request, 'cursor': page['next_cursor']})

    def test_tool_oracle_rejects_identity_outcome_and_missing_end_mutations(self):
        operation = {'name':'probe','id':'same','scope':'east','outcome':'failure','start':['fixture',1,None],'end':['fixture',2,None],'duration':1250}
        records = []
        for phase, source in zip(('start','end'), (operation['start'], operation['end'])):
            records.append({'occurrence':{'evidence_ref':source}, 'fields':{'classification':{'status':'event','semantics':{'kind':'request','name':'probe','correlation_id':'same','scope':['east'],'phase':phase,'outcome':'failure' if phase == 'end' else None}}}})
        member = {'identity':[{'field':key,'value':value} for key,value in {'kind':'Request','name':'probe','correlation_id':'same','scope':'["east"]'}.items()], 'source_occurrences':[{'evidence_ref':operation['start']},{'evidence_ref':operation['end']}], 'measurement_ids':['elapsed']}
        artifact = {'records':records,'memberships':[{'population_id':'scope-0-paired-lifecycles','members':[member]}], 'findings':[{'kind':'measurement','claim':'Elapsed','details':{'value':1250}}]}
        truth = {'operations':[operation]}
        classification_truth(artifact, truth, tuple)
        wrong = copy.deepcopy(artifact)
        wrong['records'][1]['fields']['classification']['semantics']['outcome'] = 'success'
        with self.assertRaisesRegex(AssertionError, 'outcome differs'):classification_truth(wrong, truth, tuple)
        wrong = copy.deepcopy(artifact)
        wrong['memberships'][0]['members'][0]['identity'][1]['value'] = 'other'
        with self.assertRaisesRegex(AssertionError, 'identity differs'):classification_truth(wrong, truth, tuple)
        incomplete = {'records':[records[0]],'memberships':[], 'findings':[{'kind':'observation','claim':'A start has no observed end in this selected capture; this does not establish a hang.','details':{'supporting_occurrences':[{'evidence_ref':operation['start']}]}}]}
        classification_truth(incomplete, {'operations':[]}, tuple, [operation['start']])
        incomplete['findings'][0]['details']['supporting_occurrences'][0]['evidence_ref'] = operation['end']
        with self.assertRaisesRegex(AssertionError, 'missing-end evidence differs'):classification_truth(incomplete, {'operations':[]}, tuple, [operation['start']])

    def test_tool_oracle_rejects_offset_loss_with_unchanged_elapsed_time(self):
        start, end = ['fixture',1,None], ['fixture',2,None]
        times = ['2026-06-04T12:30:00+03:00','2026-06-04T12:30:01.250+03:00']
        artifact = {'records':[{'occurrence':{'evidence_ref':source},'timestamp':time} for source,time in zip((start,end),times)], 'findings':[{'kind':'measurement','details':{'value':1250,'boundaries':{phase:{'occurrence':{'evidence_ref':source},'timestamp':time} for phase,source,time in zip(('start','end'),(start,end),times)}}}]}
        truth = {'rows':[{'source':source,'fields':{'ts':time}} for source,time in zip((start,end),times)], 'operations':[{'start':start,'end':end,'start_time':times[0],'end_time':times[1]}]}
        timestamp_truth(artifact, truth, tuple)
        altered = copy.deepcopy(artifact)
        altered['records'][0]['timestamp'] = '2026-06-04T09:30:00Z'
        with self.assertRaisesRegex(AssertionError, 'offset differs'):timestamp_truth(altered, truth, tuple)
        altered = copy.deepcopy(artifact)
        altered['findings'][0]['details']['boundaries']['end']['timestamp'] = '2026-06-04T09:30:01.250Z'
        with self.assertRaisesRegex(AssertionError, 'offset differs'):timestamp_truth(altered, truth, tuple)
