<script lang="ts">
	let { label, value, detail }: { label: string; value: string; detail?: string } = $props();
</script>

<div class="min-h-30 rounded border border-line bg-surface p-4">
	<p class="text-xs tracking-wide text-muted uppercase">{label}</p>
	<p class="mt-1 text-2xl font-bold tabular-nums">{value}</p>
	{#if detail}
		<p class="mt-1 text-xs text-muted">{detail}</p>
	{/if}
</div>
