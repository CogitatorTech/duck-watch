<script lang="ts">
	import type { Snippet } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { logout } from '$lib/services/api';
	import {
		getThemeMode,
		setThemeMode,
		startTheme,
		type ThemeMode,
	} from '$lib/services/theme.svelte';
	import { endSession, getUser, isLoggedIn } from '$lib/services/session.svelte';
	import '../app.css';

	let { children }: { children: Snippet } = $props();

	// Applies the stored theme and follows the system while the mode is
	// `system`.
	$effect(() => startTheme());

	const themes: { value: ThemeMode; label: string; title: string }[] = [
		{ value: 'system', label: 'Auto', title: 'Follow the system theme' },
		{ value: 'light', label: 'Light', title: 'Always light' },
		{ value: 'dark', label: 'Dark', title: 'Always dark' },
	];

	const signOut = async () => {
		try {
			await logout();
		} catch {
			// The session ends locally even if the request fails.
		}
		endSession();
		await goto(resolve('/login'));
	};
</script>

<svelte:head>
	<title>{page.data.title ?? 'Dashboard'} | DuckWatch</title>
</svelte:head>

<div class="min-h-screen bg-canvas text-ink">
	<header class="border-b border-line bg-surface">
		<div
			class="mx-auto flex max-w-5xl flex-wrap items-center justify-between gap-y-2 px-4 py-4"
		>
			<a href={resolve('/')} class="text-lg font-bold text-accent-strong">DuckWatch</a>
			{#if isLoggedIn()}
				<nav class="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
					<a href={resolve('/')} class="text-muted hover:text-accent-strong">Dashboard</a>
					<a href={resolve('/connections')} class="text-muted hover:text-accent-strong">
						Connections
					</a>
					<span class="hidden text-faint sm:inline">{getUser()?.email}</span>
					<button class="text-muted hover:text-accent-strong" onclick={signOut}
						>Sign out</button
					>
					<select
						value={getThemeMode()}
						onchange={(event) => setThemeMode(event.currentTarget.value as ThemeMode)}
						class="rounded border border-line bg-surface px-2 py-1 text-sm"
						aria-label="Theme"
					>
						{#each themes as option (option.value)}
							<option value={option.value} title={option.title}>{option.label}</option
							>
						{/each}
					</select>
				</nav>
			{:else}
				<div class="flex items-center gap-3">
					<span class="text-sm text-muted">MotherDuck observability</span>
					<select
						value={getThemeMode()}
						onchange={(event) => setThemeMode(event.currentTarget.value as ThemeMode)}
						class="rounded border border-line bg-surface px-2 py-1 text-sm"
						aria-label="Theme"
					>
						{#each themes as option (option.value)}
							<option value={option.value} title={option.title}>{option.label}</option
							>
						{/each}
					</select>
				</div>
			{/if}
		</div>
	</header>
	<main class="mx-auto max-w-5xl px-4 py-8">
		{@render children()}
	</main>
</div>
