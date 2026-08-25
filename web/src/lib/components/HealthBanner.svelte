<script lang="ts">
	import type { ConnectionStatus } from '$lib/services/api/connections';
	import { describeHealth } from '$lib/services/health';

	let { connection }: { connection: ConnectionStatus } = $props();

	let notice = $derived(describeHealth(connection));

	// Every state keeps the same frame and the same two lines, so the page
	// does not shift when a connection starts or stops keeping up.
	const FRAMES: Record<string, string> = {
		ok: 'border-line bg-surface',
		neutral: 'border-line bg-surface',
		warn: 'border-warn-line bg-warn-surface',
		danger: 'border-danger-line bg-danger-surface',
	};
	const INKS: Record<string, string> = {
		ok: 'text-ok-ink',
		neutral: 'text-muted',
		warn: 'text-warn-ink',
		danger: 'text-danger-ink',
	};
	const DOTS: Record<string, string> = {
		ok: 'bg-ok-ink',
		neutral: 'bg-faint',
		warn: 'bg-warn-ink',
		danger: 'bg-danger-ink',
	};
</script>

<section
	class="rounded-lg border px-4 py-3 {FRAMES[notice.tone]}"
	aria-live="polite"
	aria-label="Ingestion status"
>
	<p class="flex flex-wrap items-baseline gap-x-2 gap-y-1 text-sm">
		<span class="flex items-center gap-2 font-semibold {INKS[notice.tone]}">
			<span class="size-2 shrink-0 rounded-full {DOTS[notice.tone]}" aria-hidden="true"
			></span>
			{notice.label}
		</span>
		<span class="text-muted">{notice.detail}</span>
	</p>

	<!--
		This line is what stops old numbers reading as current, so it belongs
		in the banner rather than in a tooltip.
	-->
	{#if notice.consequence}
		<p class="mt-1 text-sm {INKS[notice.tone]}">{notice.consequence}</p>
	{/if}

	{#if notice.error}
		<p
			class="mt-2 border-t border-current/20 pt-2 font-mono text-xs break-words {INKS[
				notice.tone
			]}"
		>
			{notice.error}
		</p>
	{/if}
</section>
