import { apiFetch, withJson } from '.';
import type { SessionUser } from '$lib/services/session.svelte';

export type AuthResponse = {
	user: SessionUser;
	token: string;
};

export type Account = {
	user: SessionUser;
	organization: {
		id: string;
		name: string;
		created_at: string;
		updated_at: string;
	};
};

export const signup = async (body: { org_name: string; email: string; password: string }) =>
	await apiFetch<AuthResponse>('/auth/signup', withJson({ method: 'POST' }, body));

export const login = async (body: { email: string; password: string }) =>
	await apiFetch<AuthResponse>('/auth/login', withJson({ method: 'POST' }, body));

export const logout = async () => await apiFetch('/auth/logout', { method: 'POST' });

export const getAccount = async (fetcher?: typeof fetch) =>
	await apiFetch<Account>('/me', { method: 'GET' }, fetcher);
