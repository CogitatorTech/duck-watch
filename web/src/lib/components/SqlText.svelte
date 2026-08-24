<script lang="ts">
	import { tokenizeSql } from '$lib/services/sql';

	let { sql }: { sql: string } = $props();

	const colors = {
		keyword: 'font-medium text-sql-keyword',
		string: 'text-sql-string',
		number: 'text-sql-number',
		comment: 'text-sql-comment italic',
		plain: '',
	};

	let tokens = $derived(tokenizeSql(sql));
</script>

{#each tokens as token, index (index)}<span class={colors[token.kind]}>{token.text}</span>{/each}
