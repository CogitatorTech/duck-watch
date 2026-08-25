<script lang="ts">
	import Button from '$lib/components/atoms/Button.svelte';
	import type { LatencyBucket } from '$lib/services/api/dashboard';
	import {
		describeChart,
		formatBucketWidth,
		formatMs,
		formatUsd,
		layoutChart,
	} from '$lib/services/chart';
	import { formatTimestamp } from '$lib/services/time.svelte';

	let {
		buckets,
		measure = 'latency',
		onSelect,
		emptyMessage = 'No queries in this window yet.',
		loading = false,
		error = null,
		onRetry,
	}: {
		buckets: LatencyBucket[];
		/** Bar height: how many queries ran, or what they cost. */
		measure?: 'latency' | 'cost';
		/** Called with a bucket so the page can zoom the range into it. */
		onSelect?: (bucket: LatencyBucket) => void;
		emptyMessage?: string;
		loading?: boolean;
		/** A load failure is its own state, not an empty chart. */
		error?: string | null;
		onRetry?: () => void;
	} = $props();

	const WIDTH = 600;
	const HEIGHT = 160;

	let layout = $derived(layoutChart(buckets, WIDTH, HEIGHT, measure));

	const formatValue = (value: number) =>
		measure === 'cost' ? formatUsd(value) : `${Math.round(value)} queries`;

	// Bucket width is only knowable from the spacing between two buckets.
	let bucketMs = $derived(
		buckets.length > 1
			? new Date(buckets[1].bucket_start).getTime() -
					new Date(buckets[0].bucket_start).getTime()
			: null,
	);

	let description = $derived(describeChart(buckets, measure, formatValue, formatTimestamp));

	const barTitle = (bucket: LatencyBucket) =>
		measure === 'cost'
			? `${formatTimestamp(bucket.bucket_start)}: ${formatUsd(bucket.estimated_cost_usd)} over ${bucket.query_count} queries`
			: `${formatTimestamp(bucket.bucket_start)}: ${bucket.query_count} queries, ${bucket.failure_count} failed, p95 ${formatMs(bucket.p95_ms)}`;
</script>

{#if error}
	<div class="py-8 text-center">
		<p class="text-sm text-danger">{error}</p>
		{#if onRetry}
			<div class="mt-3">
				<Button variant="secondary" size="sm" onclick={onRetry}>Try again</Button>
			</div>
		{/if}
	</div>
{:else if loading && buckets.length === 0}
	<!-- Placeholder bars rather than an empty axis frame, which reads as zero. -->
	<div class="flex h-40 items-end gap-1" aria-hidden="true">
		{#each Array.from({ length: 24 }, (_, index) => index) as index (index)}
			<div
				class="flex-1 animate-pulse rounded-t bg-line"
				style="height: {30 + ((index * 37) % 60)}%"
			></div>
		{/each}
	</div>
	<p class="mt-2 text-xs text-muted">Loading the chart...</p>
{:else if buckets.length === 0}
	<p class="py-8 text-center text-sm text-muted">{emptyMessage}</p>
{:else}
	<div class="flex gap-2">
		<!-- Axis labels live outside the SVG: the chart stretches to fit its
		     box, which would distort any text drawn inside it. -->
		<div class="flex h-40 w-16 shrink-0 flex-col justify-between text-right text-xs text-faint">
			<span>
				{measure === 'cost' ? formatUsd(layout.maxValue) : Math.round(layout.maxValue)}
			</span>
			<span>{measure === 'cost' ? '$0' : '0'}</span>
		</div>
		<div class="min-w-0 flex-1">
			<svg
				viewBox="0 0 {WIDTH} {HEIGHT}"
				class="h-40 w-full"
				role="img"
				aria-label={description}
				preserveAspectRatio="none"
			>
				<!-- Low contrast rules, so they never compete with the data. -->
				<line x1="0" y1="0" x2={WIDTH} y2="0" class="stroke-line" stroke-width="1" />
				<line
					x1="0"
					y1={HEIGHT / 2}
					x2={WIDTH}
					y2={HEIGHT / 2}
					class="stroke-line"
					stroke-width="1"
				/>
				{#each layout.bars as bar (bar.bucket.bucket_start)}
					{@const fill =
						bar.bucket.failure_count > 0 ? 'fill-chart-bar-failed' : 'fill-chart-bar'}
					{#if onSelect}
						<rect
							x={bar.x}
							y={HEIGHT - bar.height}
							width={bar.width}
							height={bar.height}
							class="{fill} cursor-pointer focus-visible:stroke-accent-strong"
							stroke-width="2"
							role="button"
							tabindex="0"
							aria-label="Zoom into {barTitle(bar.bucket)}"
							onclick={() => onSelect(bar.bucket)}
							onkeydown={(event) => {
								if (event.key === 'Enter' || event.key === ' ') {
									event.preventDefault();
									onSelect(bar.bucket);
								}
							}}
						>
							<title>{barTitle(bar.bucket)}</title>
						</rect>
					{:else}
						<rect
							x={bar.x}
							y={HEIGHT - bar.height}
							width={bar.width}
							height={bar.height}
							class={fill}
						>
							<title>{barTitle(bar.bucket)}</title>
						</rect>
					{/if}
				{/each}
				{#if measure === 'latency'}
					<!--
						One line per run of measured buckets. A single measured
						bucket gets a dot, because a one point line draws
						nothing and the reader would see no p95 at all.
					-->
					{#each layout.p95Segments as segment, index (index)}
						{#if segment.length > 1}
							<polyline
								points={segment.map((point) => `${point.x},${point.y}`).join(' ')}
								class="fill-none stroke-chart-line stroke-2"
								vector-effect="non-scaling-stroke"
							/>
						{:else}
							<circle
								cx={segment[0].x}
								cy={segment[0].y}
								r="3"
								class="fill-chart-line"
								vector-effect="non-scaling-stroke"
							/>
						{/if}
					{/each}
				{/if}
			</svg>
			<div class="mt-1 flex justify-between text-xs text-faint">
				<span>{formatTimestamp(buckets[0].bucket_start)}</span>
				<span>{formatTimestamp(buckets[buckets.length - 1].bucket_start)}</span>
			</div>
		</div>
		{#if measure === 'latency'}
			<div
				class="flex h-40 w-16 shrink-0 flex-col justify-between text-left text-xs text-chart-line"
			>
				<span>{formatMs(layout.maxP95)}</span>
				<span>0</span>
			</div>
		{/if}
	</div>

	<!-- The same numbers as a table, since a chart alone is not readable by
	     assistive technology. -->
	<div class="sr-only">
		<table>
			<caption>{description}</caption>
			<thead>
				<tr>
					<th scope="col">Bucket start</th>
					<th scope="col">{measure === 'cost' ? 'Estimated cost' : 'Queries'}</th>
					<th scope="col">Failures</th>
					<th scope="col">p95 latency</th>
				</tr>
			</thead>
			<tbody>
				{#each buckets as bucket (bucket.bucket_start)}
					<tr>
						<th scope="row">{formatTimestamp(bucket.bucket_start)}</th>
						<td>
							{measure === 'cost'
								? formatUsd(bucket.estimated_cost_usd)
								: bucket.query_count}
						</td>
						<td>{bucket.failure_count}</td>
						<td>{formatMs(bucket.p95_ms)}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>

	<div class="mt-2 flex flex-wrap justify-between gap-2 text-xs text-muted">
		{#if measure === 'cost'}
			<span>Bars show estimated cost in US dollars (red when a bucket has failures).</span>
		{:else}
			<span>
				Bars are query counts on the left axis (red when a bucket has failures); the line is
				p95 latency on the right axis.
			</span>
		{/if}
		<span>
			{#if layout.populatedBuckets > 0 && layout.populatedBuckets < buckets.length / 4}
				Queries ran in {layout.populatedBuckets} of {buckets.length} buckets &middot;
			{/if}
			{formatBucketWidth(bucketMs)}
		</span>
	</div>
	{#if onSelect}
		<p class="mt-1 text-xs text-faint">Select a bar to zoom the range into that bucket.</p>
	{/if}
{/if}
