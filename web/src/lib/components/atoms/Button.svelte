<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes } from 'svelte/elements';

	type Props = HTMLButtonAttributes & {
		variant?: 'primary' | 'secondary' | 'danger';
		size?: 'sm' | 'md';
		children: Snippet;
	};

	let {
		variant = 'primary',
		size = 'md',
		class: className = '',
		children,
		...rest
	}: Props = $props();

	/*
	 * Pressing draws a darker edge along the bottom and then tightens it, so a
	 * button reads as pushed down. The edge is an inset shadow rather than a
	 * border, so it costs no layout and nothing beside the button moves.
	 */
	const variants = {
		primary:
			'bg-accent text-accent-contrast hover:bg-accent-strong hover:shadow-[inset_0_-3px_var(--color-accent-shade)] active:shadow-[inset_0_-2px_var(--color-accent-shade)]',
		secondary:
			'bg-neutral-button text-ink hover:bg-surface-alt hover:shadow-[inset_0_0_0_1px_var(--color-line),inset_0_-3px_var(--color-line)] active:shadow-[inset_0_0_0_1px_var(--color-line),inset_0_-2px_var(--color-line)]',
		danger: 'bg-danger text-accent-contrast hover:bg-danger-strong hover:shadow-[inset_0_-3px_var(--color-danger-ink)] active:shadow-[inset_0_-2px_var(--color-danger-ink)]',
	};

	const sizes = {
		sm: 'px-3 py-1 text-sm',
		md: 'px-5 py-2',
	};
</script>

<button
	class="rounded-lg font-medium transition-[background-color,box-shadow] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50 disabled:shadow-none {variants[
		variant
	]} {sizes[size]} {className}"
	{...rest}
>
	{@render children()}
</button>
