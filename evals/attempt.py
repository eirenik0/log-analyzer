"""Received evidence survives execution, budget and validation failures."""
import math
import subprocess


class BudgetExceeded(AssertionError):
    pass


class BudgetUnknown(AssertionError):
    pass


class Attempt:
    def __init__(self):
        self.answer = None
        self.answer_received = False
        self.frames = []
        self.responses_started = 0

    def start_response(self):
        self.responses_started += 1

    def receive(self, frame):
        # Observe before protocol, process-exit and deadline checks. Invalid
        # answers remain available for independent factual/validation scoring.
        if not isinstance(frame, dict):
            return
        if 'final' in frame:
            self.answer_received = True
            self.answer = frame['final']
        usage = frame.get('usage')
        self.frames.append(usage if isinstance(usage, dict) else {})

    def usage(self, source='adapter_reported_unverified'):
        responses = max(self.responses_started, len(self.frames))
        result = {'measurement_source': source, 'responses': responses}
        for name in ('tokens', 'provider_cost_usd'):
            values = []
            for frame in self.frames:
                value = frame.get(name)
                valid = (type(value) is int and value >= 0) if name == 'tokens' else (type(value) in {float, int} and value >= 0 and math.isfinite(value))
                values.append(value if valid else None)
            values.extend([None] * (responses - len(values)))
            known = [value for value in values if value is not None]
            complete = bool(values) and len(known) == len(values)
            result[name] = sum(known) if complete else None
            result[name + '_known_total'] = sum(known) if known else None
            result[name + '_completeness'] = 'complete' if complete else 'partial' if known else 'unavailable'
            result[name + '_reported_responses'] = len(known)
            result[name + '_missing_responses'] = len(values) - len(known)
        return result


def evaluate_attempt(attempt, execute, grade, validate, checkpoint):
    """Execution status and factual quality describe different properties."""
    execution, budget, validation, errors = 'completed', 'within_limits', 'not_performed', []
    try:
        execute()
        checkpoint()
    except Exception as error:
        execution = 'failed'
        budget = 'exceeded' if isinstance(error, (BudgetExceeded, subprocess.TimeoutExpired)) else 'unknown'
        errors.append(str(error))
    factual = {'status': 'NOT_SCORED', 'errors': ['No answer received.']}
    contracts = []
    if attempt.answer_received:
        try:
            factual = grade(attempt.answer)
        except Exception as error:
            factual = {'status': 'FAIL', 'errors': [str(error)]}
        try:
            contracts = validate(attempt.answer, factual)
            validation = 'passed'
        except Exception as error:
            validation = 'failed'
            errors.append(str(error))
    return {'execution_status': execution, 'answer_available': attempt.answer_received,
            'budget_compliance': budget, 'validation_status': validation,
            'factual_quality': factual, 'execution_errors': errors, 'investigations': contracts,
            'score': factual if execution == 'completed' and validation == 'passed' else {'status': 'ERROR', 'errors': errors or ['Attempt did not complete validation.']}}


def aggregate_usage(records):
    result = {}
    for name in ('tokens', 'provider_cost_usd'):
        usage = [r['usage'] for r in records]
        known = [u.get(name + '_known_total', u.get(name)) for u in usage]
        known = [v for v in known if v is not None]
        complete = bool(usage) and all(u.get(name) is not None for u in usage)
        result[name] = sum(u[name] for u in usage) if complete else None
        result[name + '_known_total'] = sum(known) if known else None
        result[name + '_completeness'] = 'complete' if complete else 'partial' if known else 'unavailable'
        result[name + '_incomplete_attempts'] = sum(u.get(name) is None for u in usage)
    return result


class Allocation:
    """Reported spend gates later paid calls; missing spend cannot authorize them."""
    def __init__(self, total):
        if type(total) not in {int,float} or not math.isfinite(total) or total <= 0:
            raise AssertionError('explicit positive provider allocation required')
        self.total, self.spent, self.unknown = total, 0, False

    def remaining(self):
        if self.unknown: raise BudgetUnknown('remaining provider allocation is unknown after missing reported cost')
        if self.spent >= self.total: raise BudgetExceeded('allocated provider budget exhausted')
        return self.total - self.spent

    def record(self, attempt):
        usage = attempt.usage()
        self.spent += usage['provider_cost_usd_known_total'] or 0
        if attempt.responses_started and usage['provider_cost_usd'] is None:
            self.unknown = True
