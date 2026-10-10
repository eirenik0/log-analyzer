"""Strict validator for the explicitly supported contract-1 schema vocabulary.

Unknown schema keywords fail rather than silently weakening validation. The Rust
integration cross-checks every actual emitted contract with the JSON Schema crate.
"""
import math
from datetime import datetime
import re


class InvalidValue(AssertionError):
    pass


def validate(value, schema, root=None):
    root = root or schema
    supported = {'$schema', '$defs', 'title', 'type', 'enum', 'const', 'minItems', 'properties', 'items', 'additionalProperties', 'oneOf', '$ref', 'required', 'pattern', 'minimum', 'maximum', 'maxItems', 'minLength', 'anyOf', 'allOf', 'if', 'then', 'else', 'format'}
    if set(schema) - supported:
        raise ValueError('unsupported advertised schema keyword: ' + ', '.join(sorted(set(schema) - supported)))
    def check(condition, message):
        if not condition: raise InvalidValue(message)
    if '$ref' in schema:
        check(schema['$ref'].startswith('#/'), 'external schema references unsupported')
        target = root
        for key in schema['$ref'][2:].split('/'):
            target = target[key.replace('~1', '/').replace('~0', '~')]
        validate(value, target, root)
    if 'oneOf' in schema:
        valid = 0
        for candidate in schema['oneOf']:
            try:
                validate(value, candidate, root)
                valid += 1
            except InvalidValue: pass
        check(valid == 1, 'value must match exactly one contract variant')
    if 'anyOf' in schema:
        valid = 0
        for candidate in schema['anyOf']:
            try:
                validate(value, candidate, root)
                valid += 1
            except InvalidValue: pass
        check(valid >= 1, 'value must match a contract alternative')
    for candidate in schema.get('allOf', []): validate(value, candidate, root)
    if 'if' in schema:
        try:
            validate(value, schema['if'], root)
            branch = 'then'
        except InvalidValue:
            branch = 'else'
        if branch in schema: validate(value, schema[branch], root)
    if 'type' in schema:
        types = schema['type'] if isinstance(schema['type'], list) else [schema['type']]
        matches = {'object': type(value) is dict, 'array': type(value) is list, 'string': type(value) is str, 'integer': type(value) is int, 'number': type(value) in {int, float} and math.isfinite(value), 'boolean': type(value) is bool, 'null': value is None}
        check(all(name in matches for name in types), 'unsupported schema type')
        check(any(matches[name] for name in types), 'wrong contract type')
    if 'const' in schema: check(type(value) is type(schema['const']) and value == schema['const'], 'wrong constant')
    if 'enum' in schema: check(any(type(value) is type(option) and value == option for option in schema['enum']), 'value outside contract enum')
    if isinstance(value, dict):
        check(set(schema.get('required', [])) <= set(value), 'required contract property missing')
        properties = schema.get('properties', {})
        if schema.get('additionalProperties') is False: check(set(value) <= set(properties), 'extra contract property')
        for name in set(value) & set(properties): validate(value[name], properties[name], root)
        if isinstance(schema.get('additionalProperties'), dict):
            for name in set(value) - set(properties): validate(value[name], schema['additionalProperties'], root)
    if isinstance(value, list):
        check(len(value) >= schema.get('minItems', 0), 'contract array too short')
        if 'maxItems' in schema: check(len(value) <= schema['maxItems'], 'contract array too long')
        if 'items' in schema:
            for item in value: validate(item, schema['items'], root)
    if isinstance(value, str) and 'format' in schema:
        if schema['format'] != 'date-time': raise ValueError('unsupported schema format')
        try:
            parsed = datetime.fromisoformat(value.replace('Z', '+00:00'))
            check('T' in value and parsed.tzinfo is not None, 'invalid date-time')
        except ValueError as error: raise InvalidValue('invalid date-time') from error
    if isinstance(value, str) and 'minLength' in schema: check(len(value) >= schema['minLength'], 'contract string too short')
    if isinstance(value, str) and 'pattern' in schema: check(re.search(schema['pattern'], value) is not None, 'contract pattern mismatch')
    if type(value) in {int, float} and 'minimum' in schema: check(value >= schema['minimum'], 'contract minimum violated')

    if type(value) in {int, float} and 'maximum' in schema: check(value <= schema['maximum'], 'contract maximum violated')
