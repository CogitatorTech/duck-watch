<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import Button from '$lib/components/atoms/Button.svelte';
	import Input from '$lib/components/atoms/Input.svelte';
	import { ApiError, createAccount } from '$lib/services/api';
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
			const response = await createAccount({ email, password });
			startSession(response.token, response.user);
			await goto(resolve('/connections'));
		} catch (error) {
			// A conflict means somebody already claimed this instance, which
			// is a different problem from a request that failed.
			errorMessage =
				error instanceof ApiError && error.status === 409
					? 'This DuckWatch already has an account. Sign in instead.'
					: 'Could not create the account, please try again.';
		} finally {
			pending = false;
		}
	};
</script>

<div class="mx-auto max-w-sm">
	<h1 class="text-3xl font-bold">Set up DuckWatch</h1>
	<p class="mt-1 text-sm text-muted">
		This instance has no account yet. Create one to claim it. DuckWatch has a single account, so
		this only works once.
	</p>

	<form class="mt-6 flex flex-col gap-3" onsubmit={submit}>
		<Input bind:value={email} type="email" placeholder="Email" required autocomplete="email" />
		<Input
			bind:value={password}
			type="password"
			placeholder="Password (at least 8 characters)"
			required
			minlength={8}
			maxlength={128}
			autocomplete="new-password"
		/>
		<Button type="submit" disabled={pending}>Create account</Button>
	</form>

	{#if errorMessage}
		<p class="mt-4 text-sm text-danger">{errorMessage}</p>
	{/if}

	<p class="mt-6 text-sm text-muted">
		Already set up?
		<a href={resolve('/login')} class="text-accent-strong hover:underline">Sign in</a>
	</p>
</div>
