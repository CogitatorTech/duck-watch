<script lang="ts">
	import {
		getEvent,
		type QueryEvent,
		type SortDirection,
		type SortKey,
	} from '$lib/services/api/dashboard';
	import SqlText from '$lib/components/SqlText.svelte';
	import { formatBytes, formatMs, formatUsd } from '$lib/services/chart';
	import { listedStatement, statementForCopy, wholeStatement } from '$lib/services/insights';
	import { formatTimestamp } from '$lib/services/time.svelte';
	import { previewSql } from '$lib/services/sql';
	import { truncate } from '$lib/services/utils';

	let {
		events,
		emptyMessage,
		showError = false,
		sort,
		dir,
		onSort,
	}: {
		events: QueryEvent[];
		emptyMessage: string;
		showError?: boolean;
		sort: SortKey;
		dir: SortDirection;
		onSort: (key: SortKey) => void;
	} = $props();

	let expandedId = $state<string | null>(null);
	// The list truncates long statements, so expanding fetches the full row.
	let fullEvent = $state<QueryEvent | null>(null);
	let copied = $state(false);

	const toggle = async (event: QueryEvent) => {
		copied = false;
		if (expandedId === event.md_query_id) {
			expandedId = null;
			fullEvent = null;
			return;
		}
		expandedId = event.md_query_id;
		fullEvent = null;
		try {
			const detail = await getEvent(event.connection_id, event.md_query_id);
			// Only apply if this row is still the expanded one.
			if (expandedId === event.md_query_id) fullEvent = detail;
		} catch {
			// The truncated text from the list stays on screen.
		}
	};

	const onRowKeydown = (keyEvent: KeyboardEvent, event: QueryEvent) => {
		if (keyEvent.key === 'Enter' || keyEvent.key === ' ') {
			keyEvent.preventDefault();
			toggle(event);
		}
	};

	const copyQuery = async (text: string) => {
		try {
			await navigator.clipboard.writeText(text);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			// Clipboard access can be denied; selecting the text still works.
		}
	};

	const sortIndicator = (key: SortKey) => (sort === key ? (dir === 'asc' ? '↑' : '↓') : '');

	const columnCount = $derived(showError ? 6 : 5);
</script>

{#if events.length === 0}
	<p class="py-6 text-center text-sm text-muted">{emptyMessage}</p>
{:else}
	<div class="overflow-x-auto">
		<table class="w-full table-fixed text-left text-sm">
			<thead class="border-b border-line text-xs text-muted uppercase">
				<tr class="align-bottom">
					<th class="w-8 px-2 py-2"><span class="sr-only">Details</span></th>
					<th class="px-4 py-2">Query</th>
					<th
						class="hidden w-56 px-4 py-2 sm:table-cell"
						aria-sort={sort === 'started'
							? dir === 'asc'
								? 'ascending'
								: 'descending'
							: 'none'}
					>
						<button
							class="whitespace-nowrap uppercase hover:text-accent-strong"
							onclick={() => onSort('started')}
							title="Sort by start time"
						>
							Started<span class="inline-block w-3 text-left"
								>{sortIndicator('started')}</span
							>
						</button>
					</th>
					<th
						class="w-28 px-4 py-2"
						aria-sort={sort === 'duration'
							? dir === 'asc'
								? 'ascending'
								: 'descending'
							: 'none'}
					>
						<button
							class="whitespace-nowrap uppercase hover:text-accent-strong"
							onclick={() => onSort('duration')}
							title="Sort by duration"
						>
							Duration<span class="inline-block w-3 text-left"
								>{sortIndicator('duration')}</span
							>
						</button>
					</th>
					<th class="hidden w-40 px-4 py-2 md:table-cell">User</th>
					{#if showError}
						<th class="w-36 px-4 py-2">Error</th>
					{/if}
				</tr>
			</thead>
			<tbody class="divide-y divide-line-soft">
				{#each events as event, index (event.md_query_id)}
					{@const expanded = expandedId === event.md_query_id}
					{@const detail = expanded && fullEvent ? fullEvent : event}
					<tr
						class="cursor-pointer border-l-2 hover:bg-surface-alt {expanded
							? 'border-l-accent bg-surface-sunken'
							: index % 2 === 1
								? 'border-l-transparent bg-surface-alt'
								: 'border-l-transparent'}"
						tabindex="0"
						aria-expanded={expanded}
						onclick={() => toggle(event)}
						onkeydown={(keyEvent) => onRowKeydown(keyEvent, event)}
					>
						<td class="px-2 py-2 text-center text-faint" aria-hidden="true">
							<!-- Drawn rather than typed: the geometric shape glyphs are
							     missing from some system fonts and render as a box. -->
							<svg
								viewBox="0 0 12 12"
								class="inline-block h-3 w-3 transition-transform {expanded
									? 'rotate-90'
									: ''}"
								fill="currentColor"
							>
								<path d="M4 2 L9 6 L4 10 Z" />
							</svg>
						</td>
						<td class="px-4 py-2">
							<code class="text-xs [overflow-wrap:anywhere] whitespace-pre-wrap"
								>{truncate(previewSql(event.query_text), 200)}</code
							>
							{#if event.is_internal}
								<span
									class="ml-1 rounded-full bg-neutral-button px-1.5 text-xs text-muted"
								>
									duckwatch
								</span>
							{/if}
						</td>
						<td
							class="hidden px-4 py-2 tabular-nums whitespace-nowrap text-muted sm:table-cell"
						>
							{formatTimestamp(event.start_time)}
						</td>
						<td class="px-4 py-2 tabular-nums whitespace-nowrap"
							>{formatMs(event.total_elapsed_time_ms)}</td
						>
						<td class="hidden px-4 py-2 break-words text-muted md:table-cell"
							>{event.user_name ?? '-'}</td
						>
						{#if showError}
							<td class="px-4 py-2 break-words text-danger"
								>{event.error_type ?? '-'}</td
							>
						{/if}
					</tr>
					{#if expanded}
						<tr class="border-l-2 border-l-accent bg-surface-sunken">
							<td
								colspan={columnCount}
								class="border-b-2 border-b-line px-4 pt-1 pb-4"
							>
								<div class="mb-1 flex items-center justify-between">
									<span class="text-xs text-muted">
										{fullEvent ? 'Full query' : 'Loading the full query...'}
									</span>
									<button
										class="w-28 rounded-lg border border-line bg-surface px-2 py-0.5 text-xs text-muted hover:text-accent-strong"
										onclick={(clickEvent) => {
											clickEvent.stopPropagation();
											// Until the whole row arrives, the text on
											// screen is the listing's cut copy, so what
											// goes on the clipboard has to say so.
											copyQuery(
												statementForCopy(
													fullEvent
														? wholeStatement(fullEvent.query_text)
														: listedStatement(detail.query_text),
												),
											);
										}}
									>
										{copied ? 'Copied' : 'Copy query'}
									</button>
								</div>
								<pre
									class="max-h-112 overflow-auto rounded-lg border border-line bg-surface p-3 text-xs [overflow-wrap:anywhere] whitespace-pre-wrap select-text"><SqlText
										sql={detail.query_text}
									/></pre>
								{#if detail.error_message}
									<div
										class="mt-2 rounded-lg border border-danger-line bg-danger-surface p-3 text-xs text-danger-ink"
									>
										<p class="font-medium">{detail.error_type ?? 'Error'}</p>
										<p class="mt-1 whitespace-pre-wrap select-text">
											{detail.error_message}
										</p>
									</div>
								{/if}
								<dl
									class="mt-3 grid grid-cols-2 gap-x-6 gap-y-1 text-xs sm:grid-cols-4"
								>
									<dt class="text-muted">Execution</dt>
									<dd>{formatMs(detail.execution_time_ms)}</dd>
									<dt class="text-muted">Wait</dt>
									<dd>{formatMs(detail.wait_time_ms)}</dd>
									<dt class="text-muted">Total</dt>
									<dd>{formatMs(detail.total_elapsed_time_ms)}</dd>
									<dt class="text-muted">Type</dt>
									<dd>{detail.query_type ?? '-'}</dd>
									<dt class="text-muted">Started</dt>
									<dd>{formatTimestamp(detail.start_time)}</dd>
									<dt class="text-muted">Ended</dt>
									<dd>
										{detail.end_time ? formatTimestamp(detail.end_time) : '-'}
									</dd>
									<dt class="text-muted">User</dt>
									<dd>{detail.user_name ?? '-'}</dd>
									<dt class="text-muted">Duckling</dt>
									<dd class="break-words">
										{detail.instance_type ?? '-'}{detail.duckling_id
											? ` (${detail.duckling_id})`
											: ''}
									</dd>
									<dt class="text-muted">Uploaded</dt>
									<dd>{formatBytes(detail.bytes_uploaded)}</dd>
									<dt class="text-muted">Downloaded</dt>
									<dd>{formatBytes(detail.bytes_downloaded)}</dd>
									<dt class="text-muted">Spilled</dt>
									<dd>{formatBytes(detail.bytes_spilled_to_disk)}</dd>
									<dt class="text-muted">Session</dt>
									<dd>{detail.session_name ?? '-'}</dd>
									<dt class="text-muted">Est. cost</dt>
									<dd
										title="Estimated from the Duckling size, run time, and region tier"
									>
										{formatUsd(detail.estimated_cost_usd)}
									</dd>
								</dl>
							</td>
						</tr>
					{/if}
				{/each}
			</tbody>
		</table>
	</div>
{/if}
