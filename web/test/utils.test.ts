import { describe, expect, it } from 'vitest';
import { formatInZone, truncate } from '../src/lib/services/utils';

describe('truncate', () => {
	it('leaves a short string unchanged', () => {
		expect(truncate('short', 10)).toBe('short');
	});

	it('shortens a long string and marks the cut', () => {
		expect(truncate('abcdefghij', 3)).toBe('abc...');
	});
});

describe('formatInZone', () => {
	const instant = '2026-08-23T08:56:37.829Z';

	it('names the zone so a reading is never ambiguous', () => {
		expect(formatInZone(instant, 'utc')).toContain('UTC');
	});

	it('renders the UTC clock time when asked for UTC', () => {
		// 08:56 UTC, in either a 12 or 24 hour locale.
		expect(formatInZone(instant, 'utc')).toMatch(/8:56|08:56/);
	});

	it('differs from UTC when the machine is not on UTC', () => {
		const local = formatInZone(instant, 'local');
		const utc = formatInZone(instant, 'utc');
		const onUtc = Intl.DateTimeFormat().resolvedOptions().timeZone === 'UTC';
		expect(local === utc).toBe(onUtc);
	});
});
