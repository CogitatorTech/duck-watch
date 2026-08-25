<script lang="ts">
	import type { AttributionRow } from '$lib/services/api/dashboard';
	import { formatMs, formatUsd } from '$lib/services/chart';

	let {
		rows,
		label,
		emptyMessage,
		minRows = 0,
	}: {
		rows: AttributionRow[];
		label: string;
		emptyMessage: string;
		/**
		 * Rows to keep space for, so this table does not shrink and move the
		 * lists below it when a query shape is selected.
		 */
		minRows?: number;
	} = $props();

	let reserved = $derived(Math.max(0, minRows - rows.length));

	// Below a cent, the previous period is too small to divide by: the ratio
	// explodes into six figure percentages that read as a bug.
	const BASELINE_FLOOR_USD = 0.01;
	const MAX_SHOWN_PERCENT = 999;

	/** Percentage change against the previous period, or null without a baseline. */
	const change = (row: AttributionRow): number | null => {
		if (row.previous_cost_usd < BASELINE_FLOOR_USD) return null;
		return ((row.estimated_cost_usd - row.previous_cost_usd) / row.previous_cost_usd) * 100;
	};

	const changeLabel = (value: number) => {
		const arrow = value > 0 ? '↑' : '↓';
		const size = Math.abs(value);
		return size > MAX_SHOWN_PERCENT
			? `${arrow} >${MAX_SHOWN_PERCENT}%`
			: `${arrow} ${size.toFixed(1)}%`;
	};
</script>

{#if rows.length === 0}
	<p class="py-6 text-center text-sm text-muted">{emptyMessage}</p>
{:else}
	<div class="@container overflow-x-auto rounded-lg border border-line bg-surface">
		<table class="w-full table-fixed text-left text-sm">
			<thead class="border-b border-line text-xs text-muted uppercase">
				<tr>
					<th class="px-3 py-2">{label}</th>
					<th class="w-24 px-3 py-2">Est. cost</th>
					<th class="hidden w-24 px-3 py-2 @md:table-cell">vs previous</th>
					<th class="hidden w-28 px-3 py-2 @xl:table-cell">Queries</th>
					<th class="w-32 px-3 py-2">Share</th>
				</tr>
			</thead>
			<tbody class="divide-y divide-line-soft">
				{#each rows as row, index (row.key)}
					{@const delta = change(row)}
					<tr class={index % 2 === 1 ? 'bg-surface-alt' : ''}>
						<td class="px-3 py-2 font-medium break-words">{row.key}</td>
						<td class="px-3 py-2 tabular-nums whitespace-nowrap"
							>{formatUsd(row.estimated_cost_usd)}</td
						>
						<td class="hidden px-3 py-2 tabular-nums whitespace-nowrap @md:table-cell">
							{#if delta === null}
								<span
									class="text-accent-strong"
									title="Nothing meaningful to compare against in the previous period"
								>
									New
								</span>
							{:else}
								<span class={delta > 0 ? 'text-danger' : 'text-muted'}>
									{changeLabel(delta)}
								</span>
							{/if}
						</td>
						<td
							class="hidden px-3 py-2 tabular-nums whitespace-nowrap text-muted @xl:table-cell"
						>
							{row.query_count.toLocaleString()}
							{#if row.failure_count > 0}
								<span class="text-danger">({row.failure_count} failed)</span>
							{/if}
						</td>
						<td class="px-3 py-2">
							<div class="flex items-center gap-2">
								<!-- The bar makes the ranking readable at a glance. -->
								<div
									class="h-2 min-w-0 flex-1 rounded-full bg-line"
									aria-hidden="true"
								>
									<div
										class="h-2 rounded-full bg-accent"
										style="width: {Math.max(row.cost_share * 100, 1)}%"
									></div>
								</div>
								<span class="w-12 text-right text-xs text-muted tabular-nums">
									{(row.cost_share * 100).toFixed(1)}%
								</span>
							</div>
						</td>
					</tr>
				{/each}
			</tbody>
			<!--
				Empty rows hold the height the table had before a selection
				narrowed it, so the page does not shift under the click.
			-->
			{#if reserved > 0}
				<tbody aria-hidden="true">
					{#each { length: reserved }, index (index)}
						<tr><td class="px-3 py-2 text-sm" colspan="5">&nbsp;</td></tr>
					{/each}
				</tbody>
			{/if}
		</table>
	</div>
	<p class="mt-1 text-xs text-faint">
		Total run time attributed: {formatMs(rows.reduce((sum, row) => sum + row.total_ms, 0))}.
	</p>
{/if}
