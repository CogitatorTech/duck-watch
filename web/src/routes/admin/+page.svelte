<script lang="ts">
	import Panel from '$lib/components/Panel.svelte';
	import { formatTimestamp } from '$lib/services/time.svelte';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();
</script>

<h1 class="text-3xl font-bold">Admin</h1>

<div class="mt-4">
	<Panel
		title="Organizations"
		description="Every organization on the platform, with its users and connection sync health."
	>
		{#if data.overviews.length === 0}
			<p class="py-6 text-center text-muted">No organizations yet.</p>
		{:else}
			<ul class="flex flex-col gap-4">
				{#each data.overviews as overview (overview.organization.id)}
					<li class="rounded border border-line bg-surface p-4">
						<div class="flex flex-wrap items-baseline justify-between gap-2">
							<p class="font-medium">{overview.organization.name}</p>
							<p class="text-xs text-muted">
								{overview.user_count}
								{overview.user_count === 1 ? 'user' : 'users'}, joined {formatTimestamp(
									overview.organization.created_at,
								)}
							</p>
						</div>
						{#if overview.connections.length === 0}
							<p class="mt-2 text-sm text-muted">No connections.</p>
						{:else}
							<ul class="mt-2 divide-y divide-line-soft">
								{#each overview.connections as connection (connection.id)}
									<li
										class="flex flex-wrap items-baseline justify-between gap-2 py-2"
									>
										<span class="text-sm">
											{connection.name}
											{#if !connection.enabled}
												<span class="text-faint">(disabled)</span>
											{/if}
										</span>
										{#if connection.last_sync_error}
											<span
												class="text-sm text-danger"
												title={connection.last_sync_error}
											>
												Last sync failed
											</span>
										{:else if connection.last_synced_at}
											<span class="text-sm text-muted">
												Synced {formatTimestamp(connection.last_synced_at)}
											</span>
										{:else}
											<span class="text-sm text-muted">Never synced</span>
										{/if}
									</li>
								{/each}
							</ul>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</Panel>
</div>
