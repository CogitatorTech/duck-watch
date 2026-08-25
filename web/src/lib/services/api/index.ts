import { goto } from '$app/navigation';
import { resolve } from '$app/paths';
import { endSession, getToken } from '$lib/services/session.svelte';

/*
 * Falls back to a path on this origin, which the web container's nginx proxies
 * to the backend. A published image then needs no build-time URL and runs
 * wherever it is deployed. The development server has no proxy, so it sets
 * `VITE_API_URL` in `web/.env` and that wins.
 */
const apiUrl = (import.meta.env.VITE_API_URL ?? '/api/v1').replace(/\/\s*$/, '');

export class ApiError extends Error {
	constructor(
		readonly status: number,
		message?: string,
	) {
		super(message ?? `request failed with status ${status}`);
		this.name = 'ApiError';
	}
}

/**
 * Wraps `fetch` with the API base URL, the bearer token, and error handling.
 * Inside a SvelteKit `load` function, pass that function's own `fetch` so the
 * request is tracked. A 401 ends the session and redirects to the login page.
 */
export const apiFetch = async <T = void>(
	url: string,
	options?: RequestInit,
	fetcher: typeof fetch = fetch,
): Promise<T> => {
	const token = getToken();
	const headers: HeadersInit = {
		...options?.headers,
		...(token ? { authorization: `Bearer ${token}` } : {}),
	};

	const response = await fetcher(`${apiUrl}${url}`, { mode: 'cors', ...options, headers });

	if (response.status === 401 && !url.startsWith('/auth/')) {
		endSession();
		await goto(resolve('/login'));
		throw new ApiError(response.status);
	}

	if (!response.ok) {
		let message: string | undefined;
		if (response.headers.get('content-type')?.includes('application/json')) {
			message = ((await response.json()) as { error?: string }).error;
		}
		throw new ApiError(response.status, message);
	}

	if (response.headers.get('content-type')?.includes('application/json')) {
		return (await response.json()) as T;
	}

	return undefined as T;
};

export const withJson = (options: RequestInit | undefined, data?: unknown): RequestInit => ({
	...options,
	headers: { ...options?.headers, 'content-type': 'application/json' },
	body: JSON.stringify(data),
});

export * from './auth';
export * from './connections';
export * from './dashboard';
