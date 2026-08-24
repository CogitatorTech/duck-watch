<script lang="ts">
	import Icon from './atoms/Icon.svelte';

	let { message = 'Something went wrong, please try again later.' }: { message?: string } =
		$props();
</script>

<div class="flex flex-col items-center gap-2 py-8 text-danger">
	<Icon type="exclamation-triangle" class="h-16 w-16" />
	<p>{message}</p>
</div>
