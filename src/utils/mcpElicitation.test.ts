import assert from 'node:assert/strict';
import { test } from 'node:test';
import { elicitationContent, initialValues, type McpElicitationField } from './mcpElicitation';

const base = { title: null, description: null };
const fields: McpElicitationField[] = [
    { ...base, name: 'name', required: true, kind: 'text', min_length: 2, max_length: 10, format: null, default: null },
    { ...base, name: 'age', required: false, kind: 'number', integer: true, minimum: 0, maximum: null, default: null },
    { ...base, name: 'subscribe', required: false, kind: 'boolean', default: true },
    { ...base, name: 'plan', required: true, kind: 'choice', options: [{ value: 'free', label: 'Free' }, { value: 'pro', label: 'Pro' }], multiple: false, min_items: null, max_items: null, default: ['free'] },
    { ...base, name: 'tags', required: false, kind: 'choice', options: [{ value: 'a', label: 'A' }], multiple: true, min_items: null, max_items: 1, default: [] },
];

test('defaults fill the form and empty optional fields are omitted', () => {
    const values = initialValues(fields);
    assert.deepEqual(values, { name: '', age: '', subscribe: true, plan: 'free', tags: [] });
    assert.deepEqual(elicitationContent(fields, values).invalid, ['name']);
    const { content, invalid } = elicitationContent(fields, { ...values, name: 'Ada' });
    assert.deepEqual(invalid, []);
    assert.deepEqual(content, { name: 'Ada', subscribe: true, plan: 'free' });
});

test('numbers, lengths and item counts are checked before sending', () => {
    const values = { ...initialValues(fields), name: 'Ada' };
    assert.deepEqual(elicitationContent(fields, { ...values, age: '3.5' }).invalid, ['age']);
    assert.deepEqual(elicitationContent(fields, { ...values, age: '-1' }).invalid, ['age']);
    assert.equal(elicitationContent(fields, { ...values, age: ' 36 ' }).content.age, 36);
    assert.deepEqual(elicitationContent(fields, { ...values, name: 'A' }).invalid, ['name']);
    assert.deepEqual(elicitationContent(fields, { ...values, tags: ['a', 'a'] }).invalid, ['tags']);
    assert.deepEqual(elicitationContent(fields, { ...values, tags: ['a'] }).content.tags, ['a']);
});
