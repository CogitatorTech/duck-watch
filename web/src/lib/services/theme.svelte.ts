import { browser } from '$app/environment';

/** `system` follows the operating system's light or dark preference. */
export type ThemeMode = 'system' | 'light' | 'dark';

const MODE_KEY = 'duckwatch_theme';

const readMode = (): ThemeMode => {
	if (!browser) return 'system';
	try {
		const raw = localStorage.getItem(MODE_KEY);
		return raw === 'light' || raw === 'dark' ? raw : 'system';
	} catch {
		return 'system';
	}
};

const prefersDark = () => {
	try {
		return window.matchMedia('(prefers-color-scheme: dark)').matches;
	} catch {
		return false;
	}
};

const preference = $state({ mode: readMode() });

export const getThemeMode = () => preference.mode;

/** The theme actually in effect, with `system` resolved. */
export const resolveTheme = (mode: ThemeMode): 'light' | 'dark' =>
	mode === 'system' ? (prefersDark() ? 'dark' : 'light') : mode;

const apply = (mode: ThemeMode) => {
	if (!browser) return;
	document.documentElement.dataset.theme = resolveTheme(mode);
};

export const setThemeMode = (mode: ThemeMode) => {
	preference.mode = mode;
	apply(mode);
	try {
		localStorage.setItem(MODE_KEY, mode);
	} catch {
		// Storage may be unavailable; the choice then lasts until reload.
	}
};

/**
 * Applies the stored choice and keeps following the system while the mode is
 * `system`. Returns a teardown for the layout's effect.
 */
export const startTheme = () => {
	apply(preference.mode);
	if (!browser) return () => {};

	const media = window.matchMedia('(prefers-color-scheme: dark)');
	const onChange = () => {
		if (preference.mode === 'system') apply('system');
	};
	media.addEventListener('change', onChange);
	return () => media.removeEventListener('change', onChange);
};
