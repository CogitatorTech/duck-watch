export const truncate = (str: string, size: number) =>
	str.length > size ? str.substring(0, size) + '...' : str;

/** Which clock rendered timestamps are shown on. */
export type TimeZoneMode = 'local' | 'utc';

/**
 * Formats an instant for display, always naming the zone so a reading can be
 * compared against MotherDuck, which reports its query history in UTC. The
 * locale comes from the browser rather than being pinned to one country.
 */
export const formatInZone = (timestamp: string, mode: TimeZoneMode) =>
	// Spelled out component by component because `timeZoneName` may not be
	// combined with the `dateStyle` and `timeStyle` shortcuts.
	new Intl.DateTimeFormat(undefined, {
		year: 'numeric',
		month: 'short',
		day: 'numeric',
		hour: 'numeric',
		minute: '2-digit',
		timeZoneName: 'short',
		...(mode === 'utc' ? { timeZone: 'UTC' } : {}),
	}).format(new Date(timestamp));
