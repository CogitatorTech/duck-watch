<script lang="ts">
	import { getShapeStatement, type Insights } from '$lib/services/api/dashboard';
	import { formatUsd } from '$lib/services/chart';
	import {
		findingAsText,
		insightCopy,
		insightEvidence,
		statementForCopy,
	} from '$lib/services/insights';
	import { previewSql } from '$lib/services/sql';
	import { formatTimestamp } from '$lib/services/time.svelte';
	import { truncate } from '$lib/services/utils';

	let {
		connectionId,
		insights,
		selected,
		onSelect,
		emptyMessage,
	}: {
		/** Which connection these findings belong to, for reading full statements. */
		connectionId: string;
		insights: Insights;
		/** The shape currently filtering the rest of the page, if any. */
		selected: string;
		onSelect: (fingerprint: string) => void;
		emptyMessage: string;
	} = $props();

	let hidden = $derived(insights.total - insights.findings.length);

	// Which button last copied, so only that one confirms. Cleared on a timer
	// rather than left standing, since it describes a moment.
	let copiedKey = $state('');
	let copyTimer: ReturnType<typeof setTimeout> | undefined;

	const toClipboard = async (key: string, text: string) => {
		try {
			await navigator.clipboard.writeText(text);
			copiedKey = key;
			clearTimeout(copyTimer);
			copyTimer = setTimeout(() => (copiedKey = ''), 2000);
		} catch {
			// Clipboard access can be denied; the text stays selectable.
		}
	};

	// Statements already read in full, so pressing copy twice asks once. Plain
	// storage rather than reactive: nothing renders from it.
	const fullStatements: Record<string, string> = {};

	/**
	 * The whole statement for a shape. The listing carries a cut copy, so this
	 * reads the stored one and falls back to the cut copy if that read fails.
	 */
	const fullStatement = async (fingerprint: string, listed: string): Promise<string> => {
		const known = fullStatements[fingerprint];
		if (known !== undefined) return known;
		try {
			const { example_sql } = await getShapeStatement(connectionId, fingerprint);
			fullStatements[fingerprint] = example_sql;
			return example_sql;
		} catch {
			return statementForCopy(listed);
		}
	};
</script>

{#if insights.findings.length === 0}
	<p class="py-6 text-center text-sm text-muted">{emptyMessage}</p>
{:else}
	<!--
		A count per kind, because the same problem across forty shapes is one
		thing to fix rather than forty, and the list below only has room for
		the most expensive of them.
	-->
	<ul class="mb-4 flex flex-wrap gap-2">
		{#each insights.totals as total (total.antipattern)}
			<li class="rounded border border-line bg-surface-alt px-2 py-1 text-xs">
				<span class="font-medium">{insightCopy(total.antipattern).title}</span>
				<span class="text-muted">
					&middot; {total.shapes}
					{total.shapes === 1 ? 'shape' : 'shapes'} &middot; {formatUsd(
						total.estimated_cost_usd,
					)}
				</span>
			</li>
		{/each}
	</ul>

	<ul class="@container divide-y divide-line-soft">
		{#each insights.findings as insight (`${insight.antipattern}:${insight.fingerprint}`)}
			{@const copy = insightCopy(insight.antipattern)}
			{@const active = selected === insight.fingerprint}
			<li
				class="border-l-2 px-1 py-3 first:pt-0 last:pb-0 {active
					? 'border-l-accent bg-surface-sunken'
					: 'border-l-transparent'}"
			>
				<div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
					<h3 class="text-sm font-semibold">{copy.title}</h3>
					<!--
						The cost sits beside the finding, because what it cost
						is what decides whether it is worth changing.
					-->
					<p class="text-sm tabular-nums">
						{formatUsd(insight.estimated_cost_usd)}
						<span class="text-muted">
							({(insight.cost_share * 100).toFixed(1)}% of the period)
						</span>
					</p>
				</div>

				<p class="mt-1 text-sm text-muted">{copy.explanation}</p>

				<p class="mt-1 text-sm text-muted">
					<span class="font-medium text-ink">Evidence:</span>
					{insightEvidence(insight)}.
					<span class="font-medium text-ink">Suggestion:</span>
					{copy.suggestion}
				</p>

				<button
					type="button"
					class="mt-2 block w-full cursor-pointer rounded border border-line bg-surface-sunken px-2 py-1.5 text-left hover:border-accent"
					onclick={() => onSelect(insight.fingerprint)}
					aria-pressed={active}
				>
					<code class="text-xs [overflow-wrap:anywhere] whitespace-pre-wrap">
						{truncate(previewSql(insight.example_sql), 200)}
					</code>
					<span class="mt-1 block text-xs text-faint">
						Filters the page to this shape. Last run {formatTimestamp(
							insight.last_seen,
						)}.
					</span>
				</button>

				<!--
					Copying sits outside the button above, which filters the
					page on click and so cannot hold selectable text.
				-->
				<div class="mt-2 flex flex-wrap items-center gap-2">
					<button
						type="button"
						class="w-28 rounded border border-line bg-surface px-2 py-0.5 text-xs text-muted hover:text-accent-strong"
						onclick={async () =>
							toClipboard(
								`sql:${insight.fingerprint}`,
								await fullStatement(insight.fingerprint, insight.example_sql),
							)}
					>
						{copiedKey === `sql:${insight.fingerprint}` ? 'Copied' : 'Copy query'}
					</button>
					<button
						type="button"
						class="w-28 rounded border border-line bg-surface px-2 py-0.5 text-xs text-muted hover:text-accent-strong"
						onclick={async () =>
							toClipboard(
								`finding:${insight.antipattern}:${insight.fingerprint}`,
								findingAsText(
									insight,
									formatUsd(insight.estimated_cost_usd),
									await fullStatement(insight.fingerprint, insight.example_sql),
								),
							)}
					>
						{copiedKey === `finding:${insight.antipattern}:${insight.fingerprint}`
							? 'Copied'
							: 'Copy finding'}
					</button>
				</div>
			</li>
		{/each}
	</ul>

	<p class="mt-3 text-xs text-faint">
		{#if hidden > 0}
			Showing the {insights.findings.length} most expensive of {insights.total} findings. The other
			{hidden} are counted above but not listed.
		{:else}
			All {insights.total} {insights.total === 1 ? 'finding' : 'findings'} are listed.
		{/if}
		These are signals worth checking, not conclusions, and the costs are estimates. One shape can
		raise more than one finding, so the totals above overlap. Do not add them up.
	</p>
{/if}
