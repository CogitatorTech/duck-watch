import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { listConnections } from '$lib/services/api';
import { isLoggedIn } from '$lib/services/session.svelte';
import type { PageLoad } from './$types';

export const load = (async ({ fetch }) => {
	if (!isLoggedIn()) redirect(307, resolve('/login'));

	// A backend that is unreachable must not replace the page with the
	// framework's error screen. The page has its own message and retry, so it
	// is handed an empty list and told the load failed.
	try {
		return {
			title: 'Dashboard',
			connections: await listConnections(fetch),
			loadFailed: false,
		};
	} catch {
		return { title: 'Dashboard', connections: [], loadFailed: true };
	}
}) satisfies PageLoad;
