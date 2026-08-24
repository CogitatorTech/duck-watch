import { apiFetch, withJson } from '.';

export type RegionTier = 'tier1' | 'tier2' | 'tier3';

export type Connection = {
	id: string;
	name: string;
	region_tier: RegionTier;
	enabled: boolean;
	watermark_start_time: string | null;
	/** When the poller last tried, whether or not it worked. */
	last_synced_at: string | null;
	/** When the poller last succeeded, which is what staleness is measured from. */
	last_success_at: string | null;
	last_sync_error: string | null;
	created_at: string;
	updated_at: string;
};

export type IngestionHealth = 'disabled' | 'pending' | 'failing' | 'stale' | 'healthy';

/** A connection plus how its ingestion is faring. */
export type ConnectionStatus = Connection & {
	health: IngestionHealth;
	seconds_since_success: number | null;
	seconds_behind: number | null;
	stale_after_seconds: number;
};

export const listConnections = async (fetcher?: typeof fetch) =>
	await apiFetch<ConnectionStatus[]>('/connections', { method: 'GET' }, fetcher);

export const createConnection = async (body: { name: string; token: string; region: RegionTier }) =>
	await apiFetch<Connection>('/connections', withJson({ method: 'POST' }, body));

export const deleteConnection = async (id: Connection['id']) =>
	await apiFetch(`/connections/${id}`, { method: 'DELETE' });
