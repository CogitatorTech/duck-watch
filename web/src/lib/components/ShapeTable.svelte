<script lang="ts">
	import type { ShapeStats } from '$lib/services/api/dashboard';
	import { formatMs, formatUsd } from '$lib/services/chart';
	import { previewSql } from '$lib/services/sql';
	import { formatTimestamp } from '$lib/services/time.svelte';
	import { truncate } from '$lib/services/utils';

	let {
		shapes,
		selected,
		onSelect,
		emptyMessage,
	}: {
		shapes: ShapeStats[];
		/** The shape currently filtering the rest of the page, if any. */
		selected: string;
		onSelect: (fingerprint: string) => void;
		emptyMessage: string;
	} = $props();
</script>

{#if shapes.length === 0}
	<p class="py-6 text-center text-sm text-muted">{emptyMessage}</p>
{:else}
	<div class="@container overflow-x-auto">
		<table class="w-full table-fixed text-left text-sm">
			<thead class="border-b border-line text-xs text-muted uppercase">
				<tr class="align-bottom">
					<th class="px-3 py-2">Query shape</th>
					<th class="w-20 px-3 py-2">Runs</th>
					<th class="w-24 px-3 py-2">Est. cost</th>
					<th class="hidden w-28 px-3 py-2 @lg:table-cell">Worst run</th>
					<th class="hidden w-56 px-3 py-2 @xl:table-cell">Last seen</th>
					<th class="w-32 px-3 py-2">Share</th>
				</tr>
			</thead>
			<tbody class="divide-y divide-line-soft">
				{#each shapes as shape, index (shape.fingerprint)}
					{@const active = selected === shape.fingerprint}
					<tr
						class="cursor-pointer border-l-2 hover:bg-surface-alt {active
							? 'border-l-accent bg-surface-sunken'
							: index % 2 === 1
								? 'border-l-transparent bg-surface-alt'
								: 'border-l-transparent'}"
						tabindex="0"
						aria-selected={active}
						onclick={() => onSelect(shape.fingerprint)}
						onkeydown={(event) => {
							if (event.key === 'Enter' || event.key === ' ') {
								event.preventDefault();
								onSelect(shape.fingerprint);
							}
						}}
					>
						<td class="px-3 py-2">
							<code class="text-xs [overflow-wrap:anywhere] whitespace-pre-wrap">
								{truncate(previewSql(shape.example_sql), 150)}
							</code>
							{#if shape.failure_count > 0}
								<span class="ml-1 text-xs text-danger">
									({shape.failure_count} failed)
								</span>
							{/if}
						</td>
						<td class="px-3 py-2 tabular-nums whitespace-nowrap">
							{shape.runs.toLocaleString()}
						</td>
						<td class="px-3 py-2 tabular-nums whitespace-nowrap">
							{formatUsd(shape.estimated_cost_usd)}
						</td>
						<td class="hidden px-3 py-2 tabular-nums whitespace-nowrap @lg:table-cell">
							{formatMs(shape.max_ms)}
						</td>
						<td
							class="hidden overflow-hidden px-3 py-2 tabular-nums whitespace-nowrap text-muted @xl:table-cell"
						>
							{formatTimestamp(shape.last_seen)}
						</td>
						<td class="px-3 py-2">
							<div class="flex items-center gap-2">
								<div class="h-2 min-w-0 flex-1 rounded bg-line" aria-hidden="true">
									<div
										class="h-2 rounded bg-accent"
										style="width: {Math.max(shape.cost_share * 100, 1)}%"
									></div>
								</div>
								<span class="w-12 text-right text-xs text-muted tabular-nums">
									{(shape.cost_share * 100).toFixed(1)}%
								</span>
							</div>
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="mt-2 text-xs text-faint">
		A shape groups queries that differ only in their values. Select one to narrow the tiles, the
		chart, the cost attribution, and the query tables to its runs. This list and storage stay as
		they are.
	</p>
{/if}
