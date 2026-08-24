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

	const variants = {
		primary: 'bg-accent text-accent-contrast hover:bg-accent-strong',
		secondary: 'bg-neutral-button text-ink hover:opacity-80',
		danger: 'bg-danger text-accent-contrast hover:bg-danger-strong',
	};

	const sizes = {
		sm: 'px-3 py-1 text-sm',
		md: 'px-5 py-2',
	};
</script>

<button
	class="rounded transition disabled:cursor-not-allowed disabled:opacity-50 {variants[
		variant
	]} {sizes[size]} {className}"
	{...rest}
>
	{@render children()}
</button>
