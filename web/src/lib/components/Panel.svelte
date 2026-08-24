<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		title,
		description,
		actions,
		children,
	}: {
		title?: string;
		description?: string;
		/** Controls that belong to this section, shown beside its title. */
		actions?: Snippet;
		children: Snippet;
	} = $props();
</script>

<!--
	One framed group: a titled header over its own content, so a heading and
	the thing it describes read as a single unit rather than as neighbours.
-->
<section class="rounded border border-line bg-surface">
	{#if title || actions}
		<header
			class="flex flex-wrap items-center justify-between gap-2 border-b border-line px-4 py-3"
		>
			<div class="min-w-0">
				{#if title}
					<h2 class="text-base font-semibold">{title}</h2>
				{/if}
				{#if description}
					<p class="mt-0.5 text-xs text-muted">{description}</p>
				{/if}
			</div>
			{#if actions}
				{@render actions()}
			{/if}
		</header>
	{/if}
	<div class="p-4">
		{@render children()}
	</div>
</section>
