import type { ConnectionStatus, IngestionHealth } from '$lib/services/api/connections';

/** How prominently a state should be shown, and which palette it uses. */
export type HealthTone = 'ok' | 'warn' | 'danger' | 'neutral';

export type HealthNotice = {
	tone: HealthTone;
	/** Short state name, shown in the banner's lead. */
	label: string;
	/** What is happening, in one sentence. */
	detail: string;
	/**
	 * What this means for the figures below. Empty when the connection is
	 * healthy, so the banner does not warn about nothing.
	 */
	consequence: string;
	/** The error MotherDuck or the poller reported, when there is one. */
	error: string | null;
};

const TONES: Record<IngestionHealth, HealthTone> = {
	healthy: 'ok',
	pending: 'neutral',
	disabled: 'neutral',
	stale: 'warn',
	failing: 'danger',
};

export const healthTone = (health: IngestionHealth): HealthTone => TONES[health] ?? 'neutral';

/**
 * Shows a number of seconds as a rough age. These are read at a glance rather
 * than compared closely, so one unit is enough.
 */
export const formatAge = (seconds: number): string => {
	const whole = Math.max(0, Math.round(seconds));
	if (whole < 60) return `${whole}s`;
	if (whole < 3600) return `${Math.round(whole / 60)} min`;
	if (whole < 86400) {
		const hours = Math.round(whole / 3600);
		return `${hours} ${hours === 1 ? 'hour' : 'hours'}`;
	}
	const days = Math.round(whole / 86400);
	return `${days} ${days === 1 ? 'day' : 'days'}`;
};

/**
 * Describes how ingestion is going for one connection, and what that means
 * for the numbers on the dashboard. Every figure is only as fresh as the last
 * sync that worked, so a connection that is behind has to say so instead of
 * letting old numbers look current.
 */
export const describeHealth = (status: ConnectionStatus): HealthNotice => {
	const since = status.seconds_since_success;
	const age = since === null ? null : formatAge(since);
	const behind = status.seconds_behind === null ? null : formatAge(status.seconds_behind);
	const error = status.last_sync_error;

	switch (status.health) {
		case 'healthy':
			return {
				tone: 'ok',
				label: 'Ingestion healthy',
				detail:
					behind === null
						? `Last sync ${age} ago.`
						: `Last sync ${age} ago. The newest query collected is ${behind} old.`,
				consequence: '',
				error: null,
			};
		case 'pending':
			return {
				tone: 'neutral',
				label: 'Waiting for the first sync',
				detail: 'The connection is set up. DuckWatch has not finished collecting yet.',
				consequence: 'There is nothing to show until it does.',
				error: null,
			};
		case 'disabled':
			return {
				tone: 'neutral',
				label: 'Ingestion turned off',
				detail:
					age === null
						? 'DuckWatch is not collecting from this connection.'
						: `DuckWatch is not collecting from this connection. It last worked ${age} ago.`,
				consequence: 'No new data will arrive, so the figures below will not change.',
				error: null,
			};
		case 'stale':
			return {
				tone: 'warn',
				label: 'Ingestion behind',
				detail:
					age === null
						? 'No sync has been recorded as working.'
						: `The last sync that worked was ${age} ago. Anything over ${formatAge(
								status.stale_after_seconds,
							)} counts as behind.`,
				consequence:
					'Queries run since then are missing, so the counts and costs below are too low.',
				error,
			};
		case 'failing':
			return {
				tone: 'danger',
				label: 'Ingestion failing',
				detail:
					age === null
						? 'Every sync so far has failed.'
						: `Syncs have failed since the last one that worked, ${age} ago.`,
				consequence:
					'Queries run since then are missing, so the counts and costs below are too low.',
				error,
			};
	}
};
