<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import Button from '$lib/components/atoms/Button.svelte';
	import Input from '$lib/components/atoms/Input.svelte';
	import { ApiError, login } from '$lib/services/api';
	import { startSession } from '$lib/services/session.svelte';

	let email = $state('');
	let password = $state('');
	let pending = $state(false);
	let errorMessage = $state<string | null>(null);

	const submit = async (event: SubmitEvent) => {
		event.preventDefault();
		pending = true;
		errorMessage = null;

		try {
			const response = await login({ email, password });
			startSession(response.token, response.user);
			await goto(resolve('/'));
		} catch (error) {
			errorMessage =
				error instanceof ApiError && error.status === 401
					? 'Wrong email or password.'
					: 'Could not sign in, please try again.';
		} finally {
			pending = false;
		}
	};
</script>

<div class="mx-auto max-w-sm">
	<h1 class="text-3xl font-bold">Sign in</h1>
	<p class="mt-1 text-sm text-muted">Watch your MotherDuck queries.</p>

	<form class="mt-6 flex flex-col gap-3" onsubmit={submit}>
		<Input bind:value={email} type="email" placeholder="Email" required autocomplete="email" />
		<Input
			bind:value={password}
			type="password"
			placeholder="Password"
			required
			autocomplete="current-password"
		/>
		<Button type="submit" disabled={pending}>Sign in</Button>
	</form>

	{#if errorMessage}
		<p class="mt-4 text-sm text-danger">{errorMessage}</p>
	{/if}

	<p class="mt-6 text-sm text-muted">
		Setting up a new DuckWatch?
		<a href={resolve('/setup')} class="text-accent-strong hover:underline">Create the account</a
		>
	</p>
</div>
