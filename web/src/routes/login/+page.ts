import type { PageLoad } from './$types';

export const load = (() => {
	return { title: 'Sign in' };
}) satisfies PageLoad;
