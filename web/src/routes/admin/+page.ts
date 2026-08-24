import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { listAdminOrganizations } from '$lib/services/api';
import { getUser, isLoggedIn } from '$lib/services/session.svelte';
import type { PageLoad } from './$types';

export const load = (async ({ fetch }) => {
	if (!isLoggedIn()) redirect(307, resolve('/login'));
	// The backend enforces the privilege; this only spares regular users a 403.
	if (!getUser()?.is_superadmin) redirect(307, resolve('/'));

	return {
		title: 'Admin',
		overviews: await listAdminOrganizations(fetch),
	};
}) satisfies PageLoad;
