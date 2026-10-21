import assert from 'node:assert/strict';
import { test } from 'node:test';
import en from '../../public/locales/en/translation.json';
import es from '../../public/locales/es/translation.json';

type Tree = { [key: string]: string | Tree };

function leaves(tree: Tree, prefix = ''): Map<string, string> {
    const out = new Map<string, string>();
    for (const [key, value] of Object.entries(tree)) {
        const path = prefix ? `${prefix}.${key}` : key;
        if (typeof value === 'string') out.set(path, value);
        else for (const [child, text] of leaves(value, path)) out.set(child, text);
    }
    return out;
}

const placeholders = (text: string) => [...text.matchAll(/\{\{\s*(\w+)\s*\}\}/g)].map(match => match[1]).sort();

const english = leaves(en as Tree);
const spanish = leaves(es as Tree);

test('English and Spanish have the same keys', () => {
    assert.deepEqual([...english.keys()].filter(key => !spanish.has(key)), [], 'missing in es');
    assert.deepEqual([...spanish.keys()].filter(key => !english.has(key)), [], 'missing in en');
});

test('every translation is filled in and keeps its placeholders', () => {
    for (const [key, text] of english) {
        const other = spanish.get(key) ?? '';
        assert.ok(text.trim() && other.trim(), `${key} is empty`);
        // A singular may spell its count out ("one block") in either language.
        const compared = (value: string) => placeholders(value).filter(name => !(key.endsWith('_one') && name === 'count'));
        assert.deepEqual(compared(other), compared(text), key);
        for (const [language, value] of [['en', text], ['es', other]] as const) {
            // i18next interpolates only {{name}}; a single-brace name is shown as typed.
            assert.ok(!/(^|[^{])\{\w+\}(?!\})/.test(value), `${language} ${key} uses {name} instead of {{name}}`);
        }
    }
});
