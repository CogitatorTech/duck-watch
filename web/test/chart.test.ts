import { describe, expect, it } from 'vitest';
import {
	describeChart,
	formatBucketWidth,
	formatBytes,
	formatMs,
	formatUsd,
	layoutChart,
} from '../src/lib/services/chart';
import type { LatencyBucket } from '../src/lib/services/api/dashboard';

const bucket = (count: number, p95: number | null, cost = 0): LatencyBucket => ({
	bucket_start: '2026-08-22T10:00:00Z',
	query_count: count,
	failure_count: 0,
	p50_ms: p95,
	p95_ms: p95,
	estimated_cost_usd: cost,
});

describe('layoutChart', () => {
	it('returns an empty layout for no buckets', () => {
		const layout = layoutChart([], 100, 50);
		expect(layout.bars).toEqual([]);
		expect(layout.p95Segments).toEqual([]);
	});

	it('scales the tallest bar to the full height', () => {
		const layout = layoutChart([bucket(5, 10), bucket(10, 20)], 100, 50);
		expect(layout.bars[1].height).toBe(50);
		expect(layout.bars[0].height).toBe(25);
	});

	it('spreads the bars over the width', () => {
		const layout = layoutChart([bucket(1, 1), bucket(1, 1)], 100, 50);
		expect(layout.bars[0].x).toBeGreaterThanOrEqual(0);
		expect(layout.bars[1].x).toBeGreaterThan(50);
		expect(layout.bars[1].x + layout.bars[1].width).toBeLessThanOrEqual(100);
	});

	it('scales bars by cost when asked for the cost measure', () => {
		const layout = layoutChart([bucket(100, 10, 1), bucket(1, 10, 4)], 100, 50, 'cost');
		// The cheap bucket ran the most queries, but cost drives the height.
		expect(layout.bars[1].height).toBe(50);
		expect(layout.bars[0].height).toBe(12.5);
		expect(layout.maxValue).toBe(4);
	});

	it('draws flat bars when a period cost nothing', () => {
		const layout = layoutChart([bucket(5, 10, 0), bucket(3, 10, 0)], 100, 50, 'cost');
		expect(layout.bars.every((bar) => bar.height === 0)).toBe(true);
		expect(layout.maxValue).toBe(0);
	});

	it('breaks the p95 line where a bucket has no queries', () => {
		// Joining across the gap would draw a latency trend through a period
		// when nothing ran.
		const layout = layoutChart([bucket(1, 10), bucket(1, null), bucket(1, 20)], 90, 50);

		expect(layout.p95Segments).toHaveLength(2);
		expect(layout.p95Segments[0]).toHaveLength(1);
		expect(layout.p95Segments[1]).toHaveLength(1);
		// The highest p95 sits at the top of the canvas.
		expect(layout.p95Segments[1][0].y).toBe(0);
	});

	it('keeps consecutive measured buckets in one segment', () => {
		const layout = layoutChart([bucket(1, 10), bucket(1, 20), bucket(1, 15)], 90, 50);
		expect(layout.p95Segments).toHaveLength(1);
		expect(layout.p95Segments[0]).toHaveLength(3);
	});

	it('counts how many buckets had queries', () => {
		// A chart of mostly empty slots reads as broken, so the caption says
		// how quiet the period was.
		const layout = layoutChart(
			[bucket(0, null), bucket(0, null), bucket(12, 6900), bucket(0, null)],
			100,
			50,
		);
		expect(layout.populatedBuckets).toBe(1);
	});

	it('reports nothing populated for an empty range', () => {
		expect(layoutChart([], 100, 50).populatedBuckets).toBe(0);
		expect(layoutChart([], 100, 50).p95Segments).toEqual([]);
	});
});

describe('formatMs', () => {
	it('handles missing values', () => {
		expect(formatMs(null)).toBe('-');
	});

	it('switches units as the duration grows', () => {
		expect(formatMs(250)).toBe('250 ms');
		expect(formatMs(2500)).toBe('2.5 s');
		expect(formatMs(90_000)).toBe('1.5 min');
	});
});

describe('formatUsd', () => {
	it('handles missing and zero amounts', () => {
		expect(formatUsd(null)).toBe('-');
		expect(formatUsd(0)).toBe('$0.00');
	});

	it('keeps tiny estimates legible', () => {
		expect(formatUsd(0.0004)).toBe('<$0.01');
		expect(formatUsd(1.5)).toBe('$1.50');
	});

	it('rounds large amounts and groups digits', () => {
		expect(formatUsd(12_345.67)).toBe('$12,346');
	});
});

describe('formatBytes', () => {
	it('switches units as the size grows', () => {
		expect(formatBytes(null)).toBe('-');
		expect(formatBytes(512)).toBe('512 B');
		expect(formatBytes(2048)).toBe('2.0 KB');
		expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
	});
});

describe('formatBucketWidth', () => {
	it('names the width in the largest sensible unit', () => {
		expect(formatBucketWidth(60_000)).toBe('1-minute buckets');
		expect(formatBucketWidth(30 * 60_000)).toBe('30-minute buckets');
		expect(formatBucketWidth(3 * 3_600_000)).toBe('3-hour buckets');
		expect(formatBucketWidth(2 * 86_400_000)).toBe('2-day buckets');
	});

	it('says nothing when the width is unknown', () => {
		expect(formatBucketWidth(null)).toBe('');
	});
});

describe('describeChart', () => {
	const at = (iso: string) => iso.slice(11, 16);

	it('names the peak rather than just the chart type', () => {
		const rows = [bucket(2, 10, 1), bucket(9, 10, 4), bucket(1, 10, 0.5)];
		rows[1].bucket_start = '2026-08-22T14:00:00Z';
		const text = describeChart(rows, 'cost', (v) => `$${v.toFixed(2)}`, at);
		expect(text).toContain('Estimated cost over 3 buckets');
		expect(text).toContain('peaking at $4.00');
		expect(text).toContain('14:00');
	});

	it('mentions failures only when there are some', () => {
		const clean = [bucket(2, 10, 1)];
		expect(describeChart(clean, 'latency', String, at)).not.toContain('failed');
		const failing = [bucket(2, 10, 1)];
		failing[0].failure_count = 3;
		expect(describeChart(failing, 'latency', String, at)).toContain('3 queries failed');
	});

	it('handles an empty range', () => {
		expect(describeChart([], 'cost', String, at)).toBe('No data in this range.');
	});
});
