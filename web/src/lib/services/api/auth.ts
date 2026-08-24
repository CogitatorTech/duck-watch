import { apiFetch, withJson } from '.';
import type { SessionUser } from '$lib/services/session.svelte';

export type AuthResponse = {
	user: SessionUser;
	token: string;
};

export type Account = {
	user: SessionUser;
};

/**
 * Whether this instance still needs its account. DuckWatch has one account,
 * claimed by whoever opens a fresh install first, so this is the only thing
 * that answers before anyone has signed in.
 */
export const needsSetup = async (fetcher?: typeof fetch) =>
	await apiFetch<{ needed: boolean }>('/auth/setup', { method: 'GET' }, fetcher);

/** Creates the one account. It is refused once the instance is claimed. */
export const createAccount = async (body: { email: string; password: string }) =>
	await apiFetch<AuthResponse>('/auth/setup', withJson({ method: 'POST' }, body));

export const login = async (body: { email: string; password: string }) =>
	await apiFetch<AuthResponse>('/auth/login', withJson({ method: 'POST' }, body));

export const logout = async () => await apiFetch('/auth/logout', { method: 'POST' });

export const getAccount = async (fetcher?: typeof fetch) =>
	await apiFetch<Account>('/me', { method: 'GET' }, fetcher);
