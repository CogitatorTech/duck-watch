import { browser } from '$app/environment';

export type SessionUser = {
	id: string;
	org_id: string;
	email: string;
	is_superadmin: boolean;
};

const TOKEN_KEY = 'duckwatch_token';
const USER_KEY = 'duckwatch_user';

const readUser = (): SessionUser | null => {
	if (!browser) return null;
	try {
		const raw = localStorage.getItem(USER_KEY);
		return raw ? (JSON.parse(raw) as SessionUser) : null;
	} catch {
		return null;
	}
};

const readToken = (): string | null => {
	if (!browser) return null;
	try {
		return localStorage.getItem(TOKEN_KEY);
	} catch {
		return null;
	}
};

// The bearer token lives in localStorage because the app is a static bundle
// without a server side cookie path. See the README for the tradeoff.
const session = $state({
	token: readToken(),
	user: readUser(),
});

export const getToken = () => session.token;

export const getUser = () => session.user;

export const isLoggedIn = () => session.token !== null;

export const startSession = (token: string, user: SessionUser) => {
	session.token = token;
	session.user = user;
	try {
		localStorage.setItem(TOKEN_KEY, token);
		localStorage.setItem(USER_KEY, JSON.stringify(user));
	} catch {
		// Storage may be unavailable; the session then lasts until reload.
	}
};

export const endSession = () => {
	session.token = null;
	session.user = null;
	try {
		localStorage.removeItem(TOKEN_KEY);
		localStorage.removeItem(USER_KEY);
	} catch {
		// Ignore storage failures on the way out.
	}
};
