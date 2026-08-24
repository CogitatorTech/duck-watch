import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { listConnections } from '$lib/services/api';
import { isLoggedIn } from '$lib/services/session.svelte';
import type { PageLoad } from './$types';

export const load = (async ({ fetch }) => {
	if (!isLoggedIn()) redirect(307, resolve('/login'));

	return {
		title: 'Connections',
		connections: await listConnections(fetch),
	};
}) satisfies PageLoad;
