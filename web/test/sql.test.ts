import { describe, expect, it } from 'vitest';
import { previewSql, tokenizeSql } from '../src/lib/services/sql';

const rejoin = (sql: string) =>
	tokenizeSql(sql)
		.map((token) => token.text)
		.join('');

describe('tokenizeSql', () => {
	it('round-trips the input text exactly', () => {
		const sql = "select a, 'it''s', 1.5 -- note\nfrom t /* block */ where b = \"col\"";
		expect(rejoin(sql)).toBe(sql);
	});

	it('marks keywords case-insensitively', () => {
		const tokens = tokenizeSql('SELECT x FROM t');
		expect(tokens.find((token) => token.text === 'SELECT')?.kind).toBe('keyword');
		expect(tokens.find((token) => token.text === 'FROM')?.kind).toBe('keyword');
		// Adjacent plain tokens (identifiers and whitespace) merge into one.
		expect(tokens.find((token) => token.text.includes('x'))?.kind).toBe('plain');
	});

	it('keeps strings, numbers, and comments apart', () => {
		const tokens = tokenizeSql("select 'from', 42 -- select nothing");
		expect(tokens.find((token) => token.text === "'from'")?.kind).toBe('string');
		expect(tokens.find((token) => token.text === '42')?.kind).toBe('number');
		expect(tokens.find((token) => token.text.startsWith('--'))?.kind).toBe('comment');
	});

	it('does not color keywords inside strings or comments', () => {
		const tokens = tokenizeSql("-- select\n'where'");
		expect(tokens.every((token) => token.kind !== 'keyword')).toBe(true);
	});

	it('tolerates text cut off mid string or mid comment', () => {
		expect(rejoin("select 'unterminat")).toBe("select 'unterminat");
		expect(rejoin('select 1 /* still open')).toBe('select 1 /* still open');
	});

	it('does not treat numbers inside identifiers as numbers', () => {
		const tokens = tokenizeSql('select tbiz104 from apollo');
		expect(tokens.find((token) => token.text.includes('tbiz104'))?.kind).toBe('plain');
	});
});

describe('previewSql', () => {
	it('skips leading comments so the preview shows the statement', () => {
		const sql = [
			'-- v2 counting rule, kept for comparison.',
			'-- and another note',
			'',
			'create or replace table apollo.activity as',
			'with params as (select 1)',
		].join('\n');
		expect(previewSql(sql)).toBe(
			'create or replace table apollo.activity as with params as (select 1)',
		);
	});

	it('collapses whitespace onto one line', () => {
		expect(previewSql('select\n   a,\n   b\nfrom t')).toBe('select a, b from t');
	});

	it('falls back to the text when a statement is only comments', () => {
		expect(previewSql('-- nothing but a note')).toBe('-- nothing but a note');
	});

	it('leaves a plain statement alone', () => {
		expect(previewSql('select 1')).toBe('select 1');
	});
});
