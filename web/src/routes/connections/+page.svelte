<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import Button from '$lib/components/atoms/Button.svelte';
	import Icon from '$lib/components/atoms/Icon.svelte';
	import Input from '$lib/components/atoms/Input.svelte';
	import Panel from '$lib/components/Panel.svelte';
	import {
		ApiError,
		createConnection,
		deleteConnection,
		type RegionTier,
	} from '$lib/services/api';
	import { formatTimestamp } from '$lib/services/time.svelte';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	// A MotherDuck organization lives in exactly one region, picked when it
	// was created, so naming the actual regions makes the choice obvious.
	const regions: { value: RegionTier; label: string; detail: string }[] = [
		{ value: 'tier1', label: 'US', detail: 'us-east-1, us-west-2' },
		{ value: 'tier2', label: 'Europe', detail: 'eu-central-1, eu-west-1' },
		{ value: 'tier3', label: 'Asia Pacific', detail: 'ap-northeast-1, ap-southeast-2' },
	];

	let name = $state('');
	let token = $state('');
	let region = $state<RegionTier>('tier1');
	let pending = $state(false);
	let errorMessage = $state<string | null>(null);

	const submit = async (event: SubmitEvent) => {
		event.preventDefault();
		pending = true;
		errorMessage = null;

		try {
			await createConnection({ name, token, region });
			name = '';
			token = '';
			await invalidateAll();
		} catch (error) {
			errorMessage =
				error instanceof ApiError && error.status === 422
					? (error.message ?? 'MotherDuck rejected the token.')
					: 'Could not add the connection.';
		} finally {
			pending = false;
		}
	};

	const remove = async (id: string) => {
		errorMessage = null;

		try {
			await deleteConnection(id);
			await invalidateAll();
		} catch {
			errorMessage = 'Could not delete the connection.';
		}
	};
</script>

<h1 class="text-3xl font-bold">Connections</h1>

<div class="mt-4">
	<Panel
		title="Add a connection"
		description="A read-only MotherDuck service token. Reading the query history needs a Business plan and the view query history permission. Pick the region your MotherDuck organization was created in, since it sets the rates the cost estimates use."
	>
		<form class="flex flex-col gap-2 sm:flex-row" onsubmit={submit}>
			<Input
				bind:value={name}
				placeholder="Name (such as production)"
				required
				maxlength={128}
			/>
			<Input
				bind:value={token}
				type="password"
				placeholder="MotherDuck service token"
				required
				class="sm:flex-1"
			/>
			<select
				bind:value={region}
				class="rounded-lg border border-line bg-surface px-3 py-2 text-sm"
				aria-label="MotherDuck region"
			>
				{#each regions as option (option.value)}
					<option value={option.value}>{option.label}</option>
				{/each}
			</select>
			<Button type="submit" disabled={pending || name.trim().length === 0}>
				<Icon type="plus-circle" class="mr-1 inline-block h-5 w-5 align-text-bottom" />
				{pending ? 'Checking token...' : 'Add'}
			</Button>
		</form>

		{#if errorMessage}
			<p class="mt-3 border-t border-line pt-3 text-sm text-danger">{errorMessage}</p>
		{/if}
	</Panel>
</div>

<div class="mt-6">
	<Panel title="Connected accounts">
		{#if data.connections.length === 0}
			<p class="py-6 text-center text-muted">
				No connections yet. Add your MotherDuck token above to start ingesting query
				history.
			</p>
		{:else}
			<ul class="divide-y divide-line">
				{#each data.connections as connection (connection.id)}
					<li class="flex items-center justify-between gap-4 px-4 py-3">
						<div class="min-w-0">
							<p class="truncate font-medium">
								{connection.name}
								<span class="ml-1 text-xs font-normal text-faint">
									{regions.find(
										(option) => option.value === connection.region_tier,
									)?.label ?? connection.region_tier}
								</span>
							</p>
							{#if connection.last_sync_error}
								<p
									class="truncate text-sm text-danger"
									title={connection.last_sync_error}
								>
									Last sync failed: {connection.last_sync_error}
								</p>
							{:else if connection.last_synced_at}
								<p class="text-sm text-muted">
									Synced {formatTimestamp(connection.last_synced_at)}
								</p>
							{:else}
								<p class="text-sm text-muted">Waiting for the first sync...</p>
							{/if}
						</div>
						<Button
							variant="danger"
							size="sm"
							onclick={() => remove(connection.id)}
							aria-label="Delete {connection.name}"
						>
							<Icon type="trash" class="h-4 w-4" />
						</Button>
					</li>
				{/each}
			</ul>
		{/if}
	</Panel>
</div>
