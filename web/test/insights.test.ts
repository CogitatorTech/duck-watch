import { describe, expect, it } from 'vitest';
import type { Antipattern, Insight } from '../src/lib/services/api/dashboard';
import {
	STATEMENT_LIMIT,
	findingAsText,
	groupByShape,
	shapeEvidence,
	insightCopy,
	insightEvidence,
	isStatementCut,
	statementForCopy,
} from '../src/lib/services/insights';

const ALL: Antipattern[] = [
	'select_star',
	'cross_join',
	'no_filter',
	'order_without_limit',
	'spilling',
	'repeated_runs',
];

const insight = (antipattern: Antipattern, overrides: Partial<Insight> = {}): Insight => ({
	antipattern,
	fingerprint: 'aaaa',
	example_sql: 'select * from t',
	runs: 12,
	failure_count: 0,
	total_ms: 5000,
	bytes_spilled: 0,
	estimated_cost_usd: 1.5,
	cost_share: 0.2,
	last_seen: '2026-08-23T10:00:00Z',
	...overrides,
});

describe('insightCopy', () => {
	it('has copy for every finding the backend can raise', () => {
		for (const antipattern of ALL) {
			const copy = insightCopy(antipattern);
			expect(copy.title).toBeTruthy();
			expect(copy.explanation).toBeTruthy();
			expect(copy.suggestion).toBeTruthy();
		}
	});

	it('phrases suggestions without promising a saving it cannot verify', () => {
		// The cost figures are estimates, so no copy may claim a fixed
		// reduction; a suggestion that overstates its case is worse than none.
		for (const antipattern of ALL) {
			const { suggestion, explanation } = insightCopy(antipattern);
			expect(`${suggestion} ${explanation}`).not.toMatch(/\d+\s*%/);
			expect(suggestion.toLowerCase()).not.toContain('guarantee');
		}
	});
});

describe('insightEvidence', () => {
	it('reports the spilled volume and the runs behind it', () => {
		const evidence = insightEvidence(
			insight('spilling', { bytes_spilled: 6_800_000_000, runs: 138 }),
		);
		expect(evidence).toContain('138');
		expect(evidence).toMatch(/GB|TB/);
	});

	it('counts the runs behind a repetition finding', () => {
		expect(insightEvidence(insight('repeated_runs', { runs: 500 }))).toContain('500');
	});

	it('keeps the unit singular for a single run', () => {
		expect(insightEvidence(insight('select_star', { runs: 1 }))).toContain('1 run in');
	});

	it('produces evidence for every finding', () => {
		for (const antipattern of ALL) {
			expect(insightEvidence(insight(antipattern))).toBeTruthy();
		}
	});
});

describe('statementForCopy', () => {
	it('leaves a whole statement alone', () => {
		expect(statementForCopy('select a from t')).toBe('select a from t');
		expect(isStatementCut('select a from t')).toBe(false);
	});

	it('says so when the statement was cut', () => {
		// The backend keeps 2000 characters and appends an ellipsis, so
		// anything longer than the limit is an incomplete query.
		const cut = 'x'.repeat(STATEMENT_LIMIT) + '...';
		expect(isStatementCut(cut)).toBe(true);

		const copied = statementForCopy(cut);
		expect(copied.startsWith(cut)).toBe(true);
		expect(copied).toContain('not complete');
	});

	it('treats a statement exactly at the limit as whole', () => {
		const exact = 'x'.repeat(STATEMENT_LIMIT);
		expect(isStatementCut(exact)).toBe(false);
		expect(statementForCopy(exact)).toBe(exact);
	});
});

describe('groupByShape', () => {
	it('gathers every reason for one query into a single entry', () => {
		// The panel showed the same statement three times over, once per
		// reason, with its SQL and buttons repeated each time.
		const grouped = groupByShape([
			insight('no_filter', { fingerprint: 'aaaa' }),
			insight('select_star', { fingerprint: 'aaaa' }),
			insight('order_without_limit', { fingerprint: 'bbbb' }),
		]);

		expect(grouped).toHaveLength(2);
		expect(grouped[0].fingerprint).toBe('aaaa');
		expect(grouped[0].reasons.map((r) => r.antipattern)).toEqual(['no_filter', 'select_star']);
		expect(grouped[1].reasons).toHaveLength(1);
	});

	it('keeps the order the backend chose', () => {
		// Findings arrive most expensive first, and a shape belongs where its
		// dearest finding put it.
		const grouped = groupByShape([
			insight('spilling', { fingerprint: 'dear', estimated_cost_usd: 40 }),
			insight('select_star', { fingerprint: 'cheap', estimated_cost_usd: 1 }),
			insight('no_filter', { fingerprint: 'dear', estimated_cost_usd: 40 }),
		]);

		expect(grouped.map((g) => g.fingerprint)).toEqual(['dear', 'cheap']);
		expect(grouped[0].reasons).toHaveLength(2);
	});

	it('returns nothing for nothing', () => {
		expect(groupByShape([])).toEqual([]);
	});
});

describe('shapeEvidence', () => {
	it('states the runs once, however many reasons the query raised', () => {
		// Runs belong to the query, not to each reason, so putting them in
		// every chip repeated the same number three times on one card.
		const shape = groupByShape([
			insight('no_filter', { runs: 12 }),
			insight('select_star', { runs: 12 }),
			insight('cross_join', { runs: 12 }),
		])[0];

		expect(shapeEvidence(shape)).toBe('12 runs');
	});

	it('adds the spilled volume when there is any', () => {
		const shape = groupByShape([
			insight('spilling', { runs: 9, bytes_spilled: 1_101_100_000_000 }),
		])[0];

		expect(shapeEvidence(shape)).toContain('9 runs');
		expect(shapeEvidence(shape)).toMatch(/TB|GB/);
	});

	it('keeps the unit singular for a single run', () => {
		expect(shapeEvidence(groupByShape([insight('select_star', { runs: 1 })])[0])).toBe('1 run');
	});
});

describe('findingAsText', () => {
	const text = () =>
		findingAsText(
			groupByShape([
				insight('spilling', {
					bytes_spilled: 6_800_000_000,
					runs: 138,
					cost_share: 0.182,
					example_sql: 'select * from big',
					fingerprint: 'cd0f03a5054bb680',
				}),
			])[0],
			'$12.34',
			'select * from big',
		);

	it('carries what the reader needs to act without the dashboard', () => {
		const out = text();
		expect(out).toContain('Ran out of memory');
		expect(out).toContain('$12.34');
		expect(out).toContain('18.2% of the period');
		expect(out).toContain('138');
		expect(out).toContain('cd0f03a5054bb680');
		expect(out).toContain('select * from big');
	});

	it('gives the reason and the suggestion, not just the numbers', () => {
		const out = text();
		expect(out).toContain('Why:');
		expect(out).toContain(insightCopy('spilling').suggestion);
	});

	it('puts the statement last, after a blank line', () => {
		// So a reader can select the query on its own.
		const out = text();
		expect(out.endsWith('select * from big')).toBe(true);
		expect(out).toContain('\n\nselect * from big');
	});

	it('marks a cut statement in the copied finding too', () => {
		// Only reached when the read for the whole statement failed and the
		// listing's cut copy is all there is.
		const out = findingAsText(
			groupByShape([insight('select_star')])[0],
			'$1.00',
			'x'.repeat(STATEMENT_LIMIT) + '...',
		);
		expect(out).toContain('not complete');
	});

	it('uses the statement it is given, not the one in the listing', () => {
		const out = findingAsText(
			groupByShape([insight('select_star', { example_sql: 'select * from cut...' })])[0],
			'$1.00',
			'select a, b from whole',
		);
		expect(out).toContain('select a, b from whole');
		expect(out).not.toContain('cut...');
	});
});
