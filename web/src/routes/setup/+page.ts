import type { PageLoad } from './$types';

export const load = (() => {
	return { title: 'Create account' };
}) satisfies PageLoad;
