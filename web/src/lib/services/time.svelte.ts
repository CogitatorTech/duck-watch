import { browser } from '$app/environment';
import { formatInZone, type TimeZoneMode } from '$lib/services/utils';

const MODE_KEY = 'duckwatch_timezone_mode';

const readMode = (): TimeZoneMode => {
	if (!browser) return 'local';
	try {
		return localStorage.getItem(MODE_KEY) === 'utc' ? 'utc' : 'local';
	} catch {
		return 'local';
	}
};

// One shared choice, so every table, chart, and status line agrees.
const preference = $state({ mode: readMode() });

export const getTimeZoneMode = () => preference.mode;

export const setTimeZoneMode = (mode: TimeZoneMode) => {
	preference.mode = mode;
	try {
		localStorage.setItem(MODE_KEY, mode);
	} catch {
		// Storage may be unavailable; the choice then lasts until reload.
	}
};

/** The browser's zone, for labelling the toggle. */
export const localTimeZoneName = () => {
	try {
		return Intl.DateTimeFormat().resolvedOptions().timeZone;
	} catch {
		return 'local';
	}
};

/**
 * Formats an instant on the currently selected clock. Reading the preference
 * here means a component that calls this re-renders when the choice changes.
 */
export const formatTimestamp = (timestamp: string) => formatInZone(timestamp, preference.mode);
