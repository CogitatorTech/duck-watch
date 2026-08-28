import type { ConnectionStatus, IngestionHealth } from '$lib/services/api/connections';

/** How prominently a state should be shown, and which palette it uses. */
export type HealthTone = 'ok' | 'warn' | 'danger' | 'neutral';

export type HealthNotice = {
	tone: HealthTone;
	/** Short phrase for what ingestion is doing, shown in the banner's lead. */
	label: string;
	/** What is happening, in one sentence. */
	detail: string;
	/**
	 * What this means for the figures below. Empty when the connection is
	 * healthy, so the banner does not warn about nothing.
	 */
	consequence: string;
	/** The error or warning the poller reported, when there is one. */
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
		case 'healthy': {
			const detail =
				behind === null
					? `Last sync ${age} ago.`
					: `Last sync ${age} ago. The newest query collected is ${behind} old.`;
			// Syncing works, but a past pass lost rows for good, and the
			// figures below are short by them. That is worth a warning even
			// though nothing is failing.
			if (status.last_ingest_warning) {
				return {
					tone: 'warn',
					label: 'Ingesting data',
					detail,
					consequence:
						'A sync skipped rows it could not read, so those rows are missing from the figures below.',
					error: status.last_ingest_warning,
				};
			}
			return {
				tone: 'ok',
				label: 'Ingesting data',
				detail,
				consequence: '',
				error: null,
			};
		}
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
				label: 'Not ingesting data',
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
				label: 'Falling behind',
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
				label: 'Failing to ingest data',
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
