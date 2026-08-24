import { apiFetch } from '.';

export type TimeWindow = '1h' | '24h' | '7d' | '30d';

export type SortKey = 'started' | 'duration';

export type SortDirection = 'asc' | 'desc';

export type ListOptions = {
	/** Explicit range; when set it overrides the preset window. */
	from?: string;
	to?: string;
	limit?: number;
	internal?: boolean;
	sort?: SortKey;
	dir?: SortDirection;
	q?: string;
	user?: string;
	type?: string;
	minMs?: number;
	/** Restrict to one query shape. */
	shape?: string;
};

export type FilterValues = {
	user_names: string[];
	query_types: string[];
};

export type InstanceTypeCount = {
	instance_type: string;
	query_count: number;
	total_ms: number;
	estimated_cost_usd: number;
};

export type DashboardSummary = {
	query_count: number;
	failure_count: number;
	p50_ms: number | null;
	p95_ms: number | null;
	instance_types: InstanceTypeCount[];
	estimated_cost_usd: number;
};

export type LatencyBucket = {
	bucket_start: string;
	query_count: number;
	failure_count: number;
	p50_ms: number | null;
	p95_ms: number | null;
	estimated_cost_usd: number;
};

export type AttributionRow = {
	key: string;
	query_count: number;
	failure_count: number;
	total_ms: number;
	estimated_cost_usd: number;
	/** Share of the period's cost, between 0 and 1. */
	cost_share: number;
	previous_cost_usd: number;
};

export type Attribution = {
	by_user: AttributionRow[];
	by_instance_type: AttributionRow[];
	estimated_cost_usd: number;
	previous_cost_usd: number;
};

export type QueryEvent = {
	connection_id: string;
	md_query_id: string;
	query_text: string;
	query_type: string | null;
	start_time: string;
	end_time: string | null;
	execution_time_ms: number | null;
	wait_time_ms: number | null;
	total_elapsed_time_ms: number | null;
	error_type: string | null;
	error_message: string | null;
	user_name: string | null;
	instance_type: string | null;
	duckling_id: string | null;
	session_name: string | null;
	bytes_uploaded: number | null;
	bytes_downloaded: number | null;
	bytes_spilled_to_disk: number | null;
	is_internal: boolean;
	ingested_at: string;
	estimated_cost_usd: number | null;
};

const params = (connectionId: string, window: TimeWindow, options: ListOptions = {}) => {
	const search = new URLSearchParams({ connection_id: connectionId, window });
	if (options.from) search.set('from', options.from);
	if (options.to) search.set('to', options.to);
	if (options.limit !== undefined) search.set('limit', String(options.limit));
	if (options.internal) search.set('internal', 'true');
	if (options.sort) search.set('sort', options.sort);
	if (options.dir) search.set('dir', options.dir);
	if (options.q?.trim()) search.set('q', options.q.trim());
	if (options.user) search.set('user', options.user);
	if (options.type) search.set('type', options.type);
	if (options.minMs !== undefined && options.minMs > 0)
		search.set('min_ms', String(options.minMs));
	return search.toString();
};

export const getSummary = async (
	connectionId: string,
	window: TimeWindow,
	options: ListOptions = {},
	fetcher?: typeof fetch,
) =>
	await apiFetch<DashboardSummary>(
		`/dashboard/summary?${params(connectionId, window, options)}`,
		{ method: 'GET' },
		fetcher,
	);

export const getLatencyBuckets = async (
	connectionId: string,
	window: TimeWindow,
	options: ListOptions = {},
	fetcher?: typeof fetch,
) =>
	await apiFetch<LatencyBucket[]>(
		`/dashboard/latency?${params(connectionId, window, options)}`,
		{ method: 'GET' },
		fetcher,
	);

export const getSlowQueries = async (
	connectionId: string,
	window: TimeWindow,
	options: ListOptions = {},
	fetcher?: typeof fetch,
) =>
	await apiFetch<QueryEvent[]>(
		`/dashboard/slow-queries?${params(connectionId, window, options)}`,
		{ method: 'GET' },
		fetcher,
	);

export const getFailures = async (
	connectionId: string,
	window: TimeWindow,
	options: ListOptions = {},
	fetcher?: typeof fetch,
) =>
	await apiFetch<QueryEvent[]>(
		`/dashboard/failures?${params(connectionId, window, options)}`,
		{ method: 'GET' },
		fetcher,
	);

export const getAttribution = async (
	connectionId: string,
	window: TimeWindow,
	options: ListOptions = {},
	fetcher?: typeof fetch,
) =>
	await apiFetch<Attribution>(
		`/dashboard/attribution?${params(connectionId, window, options)}`,
		{ method: 'GET' },
		fetcher,
	);

export type ShapeStats = {
	fingerprint: string;
	example_sql: string;
	runs: number;
	failure_count: number;
	total_ms: number;
	max_ms: number;
	bytes_spilled: number;
	antipatterns: Antipattern[];
	estimated_cost_usd: number;
	cost_share: number;
	last_seen: string;
};

/** Something worth reviewing, found in a query's text or in how it ran. */
export type Antipattern =
	| 'select_star'
	| 'cross_join'
	| 'no_filter'
	| 'order_without_limit'
	| 'spilling'
	| 'repeated_runs';

export type Insight = {
	antipattern: Antipattern;
	fingerprint: string;
	example_sql: string;
	runs: number;
	failure_count: number;
	total_ms: number;
	bytes_spilled: number;
	estimated_cost_usd: number;
	cost_share: number;
	last_seen: string;
};

/**
 * How many shapes raised one kind of finding, and what they cost between
 * them. A shape can raise more than one kind, so these must never be added
 * together: that would count the same shape twice.
 */
export type AntipatternTotal = {
	antipattern: Antipattern;
	shapes: number;
	estimated_cost_usd: number;
};

export type Insights = {
	findings: Insight[];
	/** How many findings there are in total, before the list was capped. */
	total: number;
	totals: AntipatternTotal[];
};

/**
 * Query patterns worth reviewing over the period, most expensive first. These
 * are signals to check rather than conclusions, so each one carries what it
 * actually cost.
 */
export const getInsights = async (
	connectionId: string,
	window: TimeWindow,
	options: ListOptions = {},
	fetcher?: typeof fetch,
) =>
	await apiFetch<Insights>(
		`/dashboard/insights?${params(connectionId, window, options)}`,
		{ method: 'GET' },
		fetcher,
	);

/** Query shapes group runs that differ only in their literals. */
export const getShapes = async (
	connectionId: string,
	window: TimeWindow,
	options: ListOptions = {},
	fetcher?: typeof fetch,
) =>
	await apiFetch<ShapeStats[]>(
		`/dashboard/shapes?${params(connectionId, window, options)}`,
		{ method: 'GET' },
		fetcher,
	);

export type StorageRow = {
	database_name: string;
	active_bytes: number;
	historical_bytes: number;
	retained_for_clone_bytes: number;
	failsafe_bytes: number;
	total_bytes: number;
	estimated_monthly_cost_usd: number;
	computed_at: string;
};

export type StorageSummary = {
	databases: StorageRow[];
	total_bytes: number;
	estimated_monthly_cost_usd: number;
	computed_at: string | null;
};

/** Storage is a level, not a range, so this takes no window. */
export const getStorage = async (connectionId: string, fetcher?: typeof fetch) =>
	await apiFetch<StorageSummary>(
		`/dashboard/storage?${new URLSearchParams({ connection_id: connectionId })}`,
		{ method: 'GET' },
		fetcher,
	);

/**
 * The choices for the filter menus over one range. Only the range is passed,
 * never the rest of the filters: a menu narrowed by its own selection would
 * drop every other option and leave no way to change it.
 */
export const getFilterValues = async (
	connectionId: string,
	window: TimeWindow,
	range: Pick<ListOptions, 'from' | 'to'> = {},
	fetcher?: typeof fetch,
) =>
	await apiFetch<FilterValues>(
		`/dashboard/filters?${params(connectionId, window, range)}`,
		{ method: 'GET' },
		fetcher,
	);

export type ShapeStatement = {
	fingerprint: string;
	example_sql: string;
	parsed: boolean;
	first_seen: string;
};

/**
 * One shape's full statement. The shape lists cut long statements to keep the
 * payload small, so copying one reads it from here instead.
 */
export const getShapeStatement = async (
	connectionId: string,
	fingerprint: string,
	fetcher?: typeof fetch,
) =>
	await apiFetch<ShapeStatement>(
		`/dashboard/shape?${new URLSearchParams({
			connection_id: connectionId,
			fingerprint,
		})}`,
		{ method: 'GET' },
		fetcher,
	);

export const getEvent = async (connectionId: string, queryId: string, fetcher?: typeof fetch) =>
	await apiFetch<QueryEvent>(
		`/dashboard/event?${new URLSearchParams({ connection_id: connectionId, query_id: queryId })}`,
		{ method: 'GET' },
		fetcher,
	);
