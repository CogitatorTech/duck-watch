import type { LatencyBucket } from '$lib/services/api/dashboard';

export type ChartBar = {
	x: number;
	width: number;
	height: number;
	bucket: LatencyBucket;
};

export type ChartPoint = {
	x: number;
	y: number;
};

export type ChartLayout = {
	bars: ChartBar[];
	/**
	 * The p95 line, split wherever a bucket had no queries. Joining across a
	 * gap would draw a latency trend through a period when nothing ran, so
	 * each run of consecutive measured buckets is its own segment.
	 */
	p95Segments: ChartPoint[][];
	/** The tallest bar's value, in the measure's own units. */
	maxValue: number;
	maxP95: number;
	/** How many buckets had at least one query, for the caption below. */
	populatedBuckets: number;
};

/** What the bars stand for: how many queries ran, or what they cost. */
export type ChartMeasure = 'latency' | 'cost';

/**
 * Lays the buckets out in a normalized viewBox. The chosen measure becomes
 * bars scaled to `height`, and p95 values become polyline points on the same
 * canvas. Kept free of Svelte so vitest can cover it.
 */
export const layoutChart = (
	buckets: LatencyBucket[],
	width: number,
	height: number,
	measure: ChartMeasure = 'latency',
): ChartLayout => {
	if (buckets.length === 0) {
		return { bars: [], p95Segments: [], maxValue: 0, maxP95: 0, populatedBuckets: 0 };
	}

	const valueOf = (bucket: LatencyBucket) =>
		measure === 'cost' ? bucket.estimated_cost_usd : bucket.query_count;
	const maxValue = Math.max(...buckets.map(valueOf), 0);
	// A zero maximum would divide by zero; flat zero bars are the honest
	// rendering of a period that cost nothing.
	const scale = maxValue > 0 ? maxValue : 1;
	const maxP95 = Math.max(...buckets.map((bucket) => bucket.p95_ms ?? 0), 1);
	const slot = width / buckets.length;
	const barWidth = Math.max(slot * 0.7, 1);

	const bars = buckets.map((bucket, index) => ({
		x: index * slot + (slot - barWidth) / 2,
		width: barWidth,
		height: (valueOf(bucket) / scale) * height,
		bucket,
	}));

	// Each run of consecutive measured buckets becomes one segment. A gap
	// ends the current segment rather than being drawn through.
	const p95Segments: ChartPoint[][] = [];
	let segment: ChartPoint[] = [];
	buckets.forEach((bucket, index) => {
		if (bucket.p95_ms === null) {
			if (segment.length > 0) p95Segments.push(segment);
			segment = [];
			return;
		}
		segment.push({
			x: index * slot + slot / 2,
			y: height - (bucket.p95_ms / maxP95) * height,
		});
	});
	if (segment.length > 0) p95Segments.push(segment);

	return {
		bars,
		p95Segments,
		maxValue,
		maxP95,
		populatedBuckets: buckets.filter((bucket) => bucket.query_count > 0).length,
	};
};

/** Formats a millisecond duration for display, switching units as it grows. */
export const formatMs = (ms: number | null): string => {
	if (ms === null) return '-';
	if (ms < 1000) return `${Math.round(ms)} ms`;
	if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
	return `${(ms / 60_000).toFixed(1)} min`;
};

/** Formats a US dollar amount, keeping small estimates legible. */
export const formatUsd = (usd: number | null): string => {
	if (usd === null) return '-';
	if (usd === 0) return '$0.00';
	if (usd < 0.01) return `<$0.01`;
	if (usd < 1000) return `$${usd.toFixed(2)}`;
	return `$${Math.round(usd).toLocaleString('en-US')}`;
};

/** Formats a byte count for display. */
export const formatBytes = (bytes: number | null): string => {
	if (bytes === null) return '-';
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
};

/** A readable name for the bucket width, so a reading is never ambiguous. */
export const formatBucketWidth = (ms: number | null): string => {
	if (ms === null) return '';
	const minutes = Math.round(ms / 60_000);
	if (minutes < 60) return `${minutes}-minute buckets`;
	const hours = minutes / 60;
	if (hours < 24) return `${Number.isInteger(hours) ? hours : hours.toFixed(1)}-hour buckets`;
	const days = hours / 24;
	return `${Number.isInteger(days) ? days : days.toFixed(1)}-day buckets`;
};

/**
 * A one sentence summary naming what the chart shows and where its peak is.
 * Screen readers get this instead of "bar chart", which says nothing.
 */
export const describeChart = (
	buckets: LatencyBucket[],
	measure: ChartMeasure,
	formatValue: (value: number) => string,
	formatTime: (iso: string) => string,
): string => {
	if (buckets.length === 0) return 'No data in this range.';

	const valueOf = (bucket: LatencyBucket) =>
		measure === 'cost' ? bucket.estimated_cost_usd : bucket.query_count;
	const peak = buckets.reduce((a, b) => (valueOf(b) > valueOf(a) ? b : a));
	const total = buckets.reduce((sum, bucket) => sum + valueOf(bucket), 0);
	const failures = buckets.reduce((sum, bucket) => sum + bucket.failure_count, 0);
	const subject = measure === 'cost' ? 'Estimated cost' : 'Query count';

	return (
		`${subject} over ${buckets.length} buckets, ` +
		`from ${formatTime(buckets[0].bucket_start)} to ${formatTime(buckets[buckets.length - 1].bucket_start)}. ` +
		`Total ${formatValue(total)}, peaking at ${formatValue(valueOf(peak))} ` +
		`at ${formatTime(peak.bucket_start)}` +
		(failures > 0 ? `. ${failures} queries failed in this range.` : '.')
	);
};
