<script lang="ts">
	import type { StorageRow } from '$lib/services/api/dashboard';
	import { formatBytes, formatUsd } from '$lib/services/chart';

	let { rows }: { rows: StorageRow[] } = $props();
</script>

{#if rows.length === 0}
	<p class="py-6 text-center text-sm text-muted">
		No storage measurements yet. They need a token permitted to view organization storage.
	</p>
{:else}
	<div class="@container overflow-x-auto">
		<table class="w-full table-fixed text-left text-sm">
			<thead class="border-b border-line text-xs text-muted uppercase">
				<tr class="align-bottom">
					<th class="px-3 py-2">Database</th>
					<th class="w-28 px-3 py-2">Total</th>
					<th class="hidden w-28 px-3 py-2 @md:table-cell">Active</th>
					<th class="hidden w-28 px-3 py-2 @lg:table-cell">Historical</th>
					<th class="hidden w-28 px-3 py-2 @xl:table-cell">Clones</th>
					<th class="w-28 px-3 py-2">Per month</th>
				</tr>
			</thead>
			<tbody class="divide-y divide-line-soft">
				{#each rows as row, index (row.database_name)}
					<tr class={index % 2 === 1 ? 'bg-surface-alt' : ''}>
						<td class="px-3 py-2 font-medium break-words">{row.database_name}</td>
						<td class="px-3 py-2 tabular-nums whitespace-nowrap">
							{formatBytes(row.total_bytes)}
						</td>
						<td
							class="hidden px-3 py-2 tabular-nums whitespace-nowrap text-muted @md:table-cell"
						>
							{formatBytes(row.active_bytes)}
						</td>
						<td
							class="hidden px-3 py-2 tabular-nums whitespace-nowrap text-muted @lg:table-cell"
						>
							{formatBytes(row.historical_bytes)}
						</td>
						<td
							class="hidden px-3 py-2 tabular-nums whitespace-nowrap text-muted @xl:table-cell"
						>
							{formatBytes(row.retained_for_clone_bytes)}
						</td>
						<td class="px-3 py-2 tabular-nums whitespace-nowrap">
							{formatUsd(row.estimated_monthly_cost_usd)}
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
{/if}
