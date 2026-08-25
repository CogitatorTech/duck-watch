<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';

	type Props = Omit<HTMLInputAttributes, 'value'> & { value?: string };

	let { value = $bindable(''), class: className = '', ...rest }: Props = $props();
</script>

<input
	bind:value
	class="rounded-lg border border-line bg-surface px-3 py-2 text-ink outline-transparent transition-[outline] invalid:border-danger focus:outline-accent {className}"
	{...rest}
/>
