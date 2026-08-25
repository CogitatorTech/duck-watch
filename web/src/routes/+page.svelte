<script lang="ts">
	import { resolve } from '$app/paths';
	import Button from '$lib/components/atoms/Button.svelte';
	import Input from '$lib/components/atoms/Input.svelte';
	import AttributionTable from '$lib/components/AttributionTable.svelte';
	import HealthBanner from '$lib/components/HealthBanner.svelte';
	import InsightList from '$lib/components/InsightList.svelte';
	import Panel from '$lib/components/Panel.svelte';
	import ShapeTable from '$lib/components/ShapeTable.svelte';
	import LatencyChart from '$lib/components/LatencyChart.svelte';
	import QueryTable from '$lib/components/QueryTable.svelte';
	import StatTile from '$lib/components/StatTile.svelte';
	import StorageTable from '$lib/components/StorageTable.svelte';
	import {
		getAttribution,
		getFailures,
		getFilterValues,
		getInsights,
		getLatencyBuckets,
		getShapes,
		getSlowQueries,
		getStorage,
		getSummary,
		listConnections,
		type Attribution,
		type DashboardSummary,
		type FilterValues,
		type Insights,
		type LatencyBucket,
		type ShapeStats,
		type StorageSummary,
		type QueryEvent,
		type SortDirection,
		type SortKey,
		type TimeWindow,
	} from '$lib/services/api';
	import { formatBytes, formatMs, formatUsd, type ChartMeasure } from '$lib/services/chart';
	import {
		formatTimestamp,
		getTimeZoneMode,
		localTimeZoneName,
		setTimeZoneMode,
	} from '$lib/services/time.svelte';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	const REFRESH_INTERVAL_MS = 30_000;
	const CONNECTION_KEY = 'duckwatch_dashboard_connection';
	const WINDOW_KEY = 'duckwatch_dashboard_window';

	const windows: { value: TimeWindow; label: string; ms: number }[] = [
		{ value: '1h', label: 'Last hour', ms: 3_600_000 },
		{ value: '24h', label: 'Last 24 hours', ms: 86_400_000 },
		{ value: '7d', label: 'Last 7 days', ms: 7 * 86_400_000 },
		{ value: '30d', label: 'Last 30 days', ms: 30 * 86_400_000 },
	];

	const readStored = (key: string): string | null => {
		try {
			return localStorage.getItem(key);
		} catch {
			return null;
		}
	};

	const store = (key: string, value: string) => {
		try {
			localStorage.setItem(key, value);
		} catch {
			// Storage may be unavailable; the picks then last until reload.
		}
	};

	const isWindow = (value: string | null): value is TimeWindow =>
		windows.some((option) => option.value === value);

	// The load data seeds the state once; refreshes keep it current after.
	// svelte-ignore state_referenced_locally
	let connections = $state(data.connections);
	// svelte-ignore state_referenced_locally
	let connectionId = $state(
		data.connections.some((connection) => connection.id === readStored(CONNECTION_KEY))
			? readStored(CONNECTION_KEY)
			: (data.connections[0]?.id ?? null),
	);
	let window = $state<TimeWindow>(
		isWindow(readStored(WINDOW_KEY)) ? (readStored(WINDOW_KEY) as TimeWindow) : '24h',
	);

	let showInternal = $state(false);
	// A custom range overrides the preset window when both ends are set.
	let fromInput = $state('');
	let toInput = $state('');
	// Free text search is debounced below, so typing does not refetch per key.
	let searchInput = $state('');
	let search = $state('');
	let userFilter = $state('');
	let typeFilter = $state('');
	let minSeconds = $state('');
	let filterValues = $state<FilterValues>({ user_names: [], query_types: [] });
	// Per-table ordering; clicking a header again flips the direction.
	let slowSort = $state<{ sort: SortKey; dir: SortDirection }>({
		sort: 'duration',
		dir: 'desc',
	});
	let failureSort = $state<{ sort: SortKey; dir: SortDirection }>({
		sort: 'started',
		dir: 'desc',
	});

	let chartMeasure = $state<ChartMeasure>('latency');
	let attribution = $state<Attribution | null>(null);
	let storage = $state<StorageSummary | null>(null);
	let shapes = $state<ShapeStats[]>([]);
	let insights = $state<Insights>({ findings: [], total: 0, totals: [] });
	// How many attribution rows there are with no shape selected. Selecting a
	// shape narrows that table, and it sits above the lists doing the
	// selecting, so its height is held to stop the page shifting under the
	// click.
	let attributionFloor = $state({ user: 0, instance: 0 });
	// Selecting a shape narrows every panel to that one family of queries.
	let shapeFilter = $state('');
	let summary = $state<DashboardSummary | null>(null);
	let buckets = $state<LatencyBucket[]>([]);
	let slowQueries = $state<QueryEvent[]>([]);
	let failures = $state<QueryEvent[]>([]);
	let loading = $state(false);
	let errorMessage = $state<string | null>(null);
	// Only the latest request may write its results, so a slow response for an
	// old selection cannot overwrite a newer one.
	let requestSequence = 0;

	let selectedConnection = $derived(
		connections.find((connection) => connection.id === connectionId) ?? null,
	);

	const refresh = async (background: boolean) => {
		if (connectionId === null) return;
		const id = connectionId;
		const win = window;
		const internal = showInternal;
		const slow = slowSort;
		const failed = failureSort;
		const minMs = Math.round((Number(minSeconds) || 0) * 1000);
		const range = customRange;
		const filters = {
			internal,
			q: search,
			user: userFilter || undefined,
			type: typeFilter || undefined,
			minMs,
			shape: shapeFilter || undefined,
			...range,
		};
		const sequence = ++requestSequence;

		if (!background) {
			loading = true;
			errorMessage = null;
		}
		try {
			const [
				nextConnections,
				nextSummary,
				nextBuckets,
				nextSlow,
				nextFailures,
				nextValues,
				nextAttribution,
				nextStorage,
				nextShapes,
				nextInsights,
			] = await Promise.all([
				listConnections(),
				getSummary(id, win, filters),
				getLatencyBuckets(id, win, filters),
				getSlowQueries(id, win, { ...filters, sort: slow.sort, dir: slow.dir }),
				getFailures(id, win, { ...filters, sort: failed.sort, dir: failed.dir }),
				// The menus follow the same range as the data below them.
				getFilterValues(id, win, range),
				getAttribution(id, win, filters),
				getStorage(id),
				// The shape list itself ignores the shape filter, so a
				// selected shape does not hide its neighbours.
				getShapes(id, win, { ...filters, shape: undefined }),
				// Findings describe the whole period, so like the shape list
				// they ignore a selected shape.
				getInsights(id, win, { ...filters, shape: undefined }),
			]);
			if (sequence !== requestSequence) return;
			connections = nextConnections;
			summary = nextSummary;
			buckets = nextBuckets;
			slowQueries = nextSlow;
			failures = nextFailures;
			filterValues = nextValues;
			attribution = nextAttribution;
			if (!shapeFilter) {
				attributionFloor = {
					user: nextAttribution.by_user.length,
					instance: nextAttribution.by_instance_type.length,
				};
			}
			storage = nextStorage;
			shapes = nextShapes;
			insights = nextInsights;
			errorMessage = null;
		} catch {
			// A failed background refresh keeps the stale data on screen.
			if (sequence === requestSequence && !background) {
				errorMessage = 'Could not load the dashboard data.';
			}
		} finally {
			if (sequence === requestSequence) loading = false;
		}
	};

	$effect(() => {
		if (connectionId !== null) store(CONNECTION_KEY, connectionId);
		store(WINDOW_KEY, window);
		refresh(false);
	});

	$effect(() => {
		const timer = setInterval(() => refresh(true), REFRESH_INTERVAL_MS);
		return () => clearInterval(timer);
	});

	// Debounce the search box into the tracked `search` state.
	$effect(() => {
		const value = searchInput;
		const timer = setTimeout(() => (search = value.trim()), 350);
		return () => clearTimeout(timer);
	});

	// An ISO instant per end, or nothing when the custom range is incomplete.
	let customRange = $derived.by(() => {
		if (!fromInput) return {};
		const from = new Date(fromInput);
		if (Number.isNaN(from.getTime())) return {};
		const to = toInput ? new Date(toInput) : new Date();
		if (Number.isNaN(to.getTime()) || to <= from) return {};
		return { from: from.toISOString(), to: to.toISOString() };
	});

	let rangeActive = $derived('from' in customRange);

	let filtersActive = $derived(
		search !== '' ||
			userFilter !== '' ||
			typeFilter !== '' ||
			shapeFilter !== '' ||
			(Number(minSeconds) || 0) > 0,
	);

	/** The local wall clock string a datetime-local input expects. */
	const toInputValue = (date: Date) => {
		const pad = (value: number) => String(value).padStart(2, '0');
		return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
	};

	// Selecting a bar narrows the range to that bucket, so the tables below
	// show the queries behind it.
	const zoomToBucket = (bucket: LatencyBucket) => {
		const start = new Date(bucket.bucket_start);
		const width =
			buckets.length > 1
				? new Date(buckets[1].bucket_start).getTime() -
					new Date(buckets[0].bucket_start).getTime()
				: 60_000;
		fromInput = toInputValue(start);
		toInput = toInputValue(new Date(start.getTime() + width));
	};

	/**
	 * Fills both ends from the active preset the first time someone reaches
	 * for the range. A date picked into an empty field would otherwise leave
	 * the time blank, which the browser treats as no value at all.
	 */
	const seedRange = () => {
		if (fromInput || toInput) return;
		const span = windows.find((option) => option.value === window)?.ms ?? 86_400_000;
		const end = new Date();
		fromInput = toInputValue(new Date(end.getTime() - span));
		toInput = toInputValue(end);
	};

	const clearRange = () => {
		fromInput = '';
		toInput = '';
	};

	/** Selecting the shape already in use clears it, so the row toggles. */
	const toggleShape = (fingerprint: string) => {
		shapeFilter = shapeFilter === fingerprint ? '' : fingerprint;
	};

	const clearFilters = () => {
		shapeFilter = '';
		searchInput = '';
		search = '';
		userFilter = '';
		typeFilter = '';
		minSeconds = '';
	};

	let instanceDetail = $derived(
		summary === null || summary.instance_types.length < 2
			? undefined
			: summary.instance_types
					.map((entry) => `${entry.instance_type}: ${entry.query_count}`)
					.join(', '),
	);

	let costDetail = $derived.by(() => {
		if (summary === null) return undefined;
		const priced = summary.instance_types.filter((entry) => entry.estimated_cost_usd > 0);
		if (priced.length === 0) return 'No priced Duckling time in this range.';
		// One size makes the breakdown a restatement of the tile.
		if (priced.length < 2) return undefined;
		return priced
			.map((entry) => `${entry.instance_type}: ${formatUsd(entry.estimated_cost_usd)}`)
			.join(', ');
	});

	const sortTable = (table: 'slow' | 'failures', key: SortKey) => {
		const current = table === 'slow' ? slowSort : failureSort;
		const next: { sort: SortKey; dir: SortDirection } =
			current.sort === key
				? { sort: key, dir: current.dir === 'desc' ? 'asc' : 'desc' }
				: { sort: key, dir: 'desc' };
		if (table === 'slow') slowSort = next;
		else failureSort = next;
	};

	let waitingForFirstSync = $derived(
		summary !== null &&
			summary.query_count === 0 &&
			selectedConnection !== null &&
			selectedConnection.health === 'pending',
	);
</script>

<!--
	The header already says where the reader is, so the heading is kept for
	assistive technology and the banner takes the top of the page.
-->
<h1 class="sr-only">Dashboard</h1>

{#if selectedConnection}
	<HealthBanner connection={selectedConnection} />
{/if}

{#if data.connections.length > 0}
	<!--
		Scope and an action rather than filters: which account is being read,
		on which clock, and a way to refetch now. These stay usable while the
		first sync is still running, so they sit outside the filters below.
	-->
	<div class="mt-4 flex flex-wrap items-center gap-2">
		<select
			bind:value={connectionId}
			class="rounded-lg border border-line bg-surface px-3 py-2 text-sm"
			aria-label="Connection"
		>
			{#each connections as connection (connection.id)}
				<option value={connection.id}>{connection.name}</option>
			{/each}
		</select>
		<select
			value={getTimeZoneMode()}
			onchange={(event) =>
				setTimeZoneMode(event.currentTarget.value === 'utc' ? 'utc' : 'local')}
			class="rounded-lg border border-line bg-surface px-3 py-2 text-sm"
			aria-label="Time zone"
			title="MotherDuck reports its query history in UTC"
		>
			<option value="local">{localTimeZoneName()}</option>
			<option value="utc">UTC</option>
		</select>
		<Button variant="secondary" size="sm" disabled={loading} onclick={() => refresh(false)}>
			Refresh
		</Button>
		<span class="w-20 text-xs text-muted" aria-live="polite">
			{loading ? 'Updating...' : ''}
		</span>
	</div>
{/if}

{#if data.connections.length === 0}
	<p class="mt-12 text-center text-muted">
		Connect a MotherDuck account first on the
		<a href={resolve('/connections')} class="text-accent-strong hover:underline"
			>connections page</a
		>.
	</p>
{:else if waitingForFirstSync}
	<p class="mt-12 text-center text-muted">
		The first sync usually lands within a minute. This page refreshes itself.
	</p>
{:else}
	<div class="mt-6">
		<Panel title="Filters">
			<!--
				The time range is a filter like any other, so it lives here
				rather than in a panel of its own.
			-->
			<div class="flex flex-wrap items-center gap-2">
				<select
					bind:value={window}
					class="rounded-lg border border-line bg-surface px-3 py-2 text-sm"
					aria-label="Time window"
				>
					{#each windows as option (option.value)}
						<option value={option.value}>{option.label}</option>
					{/each}
				</select>
				<label class="flex items-center gap-1.5 text-sm text-muted">
					From
					<Input
						bind:value={fromInput}
						type="datetime-local"
						step="60"
						onfocus={seedRange}
						class="w-56 text-sm"
						aria-label="Range start date and time"
					/>
				</label>
				<label class="flex items-center gap-1.5 text-sm text-muted">
					To
					<Input
						bind:value={toInput}
						type="datetime-local"
						step="60"
						onfocus={seedRange}
						class="w-56 text-sm"
						aria-label="Range end date and time"
					/>
				</label>
				<Button
					variant="secondary"
					size="sm"
					disabled={!fromInput && !toInput}
					onclick={clearRange}
				>
					Use preset window
				</Button>
				<!--
					Only the sentence swaps, and it trails the controls so its
					length cannot move anything.
				-->
				<span class="text-xs text-faint">
					{#if rangeActive}
						Custom range in use; the preset dropdown is ignored.
					{:else if fromInput || toInput}
						Set a start before an end to use a custom range.
					{:else}
						Empty fields use the preset window.
					{/if}
				</span>
			</div>

			<div class="mt-3 flex flex-wrap items-center gap-2 border-t border-line pt-3">
				<Input
					bind:value={searchInput}
					type="search"
					placeholder="Search query text..."
					class="min-w-40 flex-1 text-sm"
					aria-label="Search queries"
				/>
				<select
					bind:value={userFilter}
					class="rounded-lg border border-line bg-surface px-3 py-2 text-sm"
					aria-label="User"
				>
					<option value="">All users</option>
					{#each filterValues.user_names as name (name)}
						<option value={name}>{name}</option>
					{/each}
				</select>
				<select
					bind:value={typeFilter}
					class="rounded-lg border border-line bg-surface px-3 py-2 text-sm"
					aria-label="Query type"
				>
					<option value="">All types</option>
					{#each filterValues.query_types as name (name)}
						<option value={name}>{name}</option>
					{/each}
				</select>
				<label class="flex items-center gap-1.5 text-sm text-muted">
					Min duration
					<Input
						bind:value={minSeconds}
						type="number"
						min="0"
						step="0.1"
						placeholder="0"
						onwheel={(event) => event.currentTarget.blur()}
						class="w-28 text-sm"
						aria-label="Minimum duration in seconds"
					/>
					s
				</label>
				<label class="flex items-center gap-1.5 text-sm text-muted">
					<input type="checkbox" bind:checked={showInternal} class="accent-accent" />
					Show DuckWatch queries
				</label>
				<Button
					variant="secondary"
					size="sm"
					disabled={!filtersActive}
					onclick={clearFilters}
				>
					Clear filters
				</Button>
			</div>
		</Panel>
	</div>

	<div class="mt-6 grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-6" aria-busy={loading}>
		<StatTile
			label="Queries"
			value={String(summary?.query_count ?? '-')}
			detail={instanceDetail}
		/>
		<StatTile label="Failures" value={String(summary?.failure_count ?? '-')} />
		<StatTile label="p50 latency" value={formatMs(summary?.p50_ms ?? null)} />
		<StatTile label="p95 latency" value={formatMs(summary?.p95_ms ?? null)} />
		<StatTile
			label="Compute cost"
			value={formatUsd(summary?.estimated_cost_usd ?? null)}
			detail={costDetail}
		/>
		<StatTile
			label="Storage"
			value={storage ? formatBytes(storage.total_bytes) : '-'}
			detail={storage && storage.total_bytes > 0
				? `${formatUsd(storage.estimated_monthly_cost_usd)} per month`
				: undefined}
		/>
	</div>

	<div class="mt-6">
		<Panel title={chartMeasure === 'cost' ? 'Estimated cost' : 'Latency'}>
			{#snippet actions()}
				<select
					bind:value={chartMeasure}
					class="rounded-lg border border-line bg-surface px-3 py-2 text-sm"
					aria-label="Chart measure"
				>
					<option value="latency">Queries and p95 latency</option>
					<option value="cost">Estimated cost</option>
				</select>
			{/snippet}
			<LatencyChart
				{buckets}
				{loading}
				measure={chartMeasure}
				onSelect={zoomToBucket}
				error={buckets.length === 0 ? errorMessage : null}
				onRetry={() => refresh(false)}
				emptyMessage={filtersActive
					? 'No queries match the current filters.'
					: 'No queries in this window yet.'}
			/>
		</Panel>
	</div>

	<div class="mt-6">
		<Panel title="Cost attribution">
			<div class="grid gap-6 xl:grid-cols-2">
				<div>
					<h3 class="mb-2 text-sm font-medium text-muted">By user</h3>
					<AttributionTable
						rows={attribution?.by_user ?? []}
						minRows={attributionFloor.user}
						label="User"
						emptyMessage={filtersActive
							? 'No queries match the current filters.'
							: 'No attributed queries in this range.'}
					/>
				</div>
				<div>
					<h3 class="mb-2 text-sm font-medium text-muted">By Duckling size</h3>
					<AttributionTable
						rows={attribution?.by_instance_type ?? []}
						minRows={attributionFloor.instance}
						label="Duckling"
						emptyMessage={filtersActive
							? 'No queries match the current filters.'
							: 'No attributed queries in this range.'}
					/>
				</div>
			</div>
		</Panel>
	</div>

	<div class="mt-6">
		<Panel title="Storage">
			<StorageTable rows={storage?.databases ?? []} />
			{#if storage?.computed_at}
				<p class="mt-2 text-xs text-faint">
					These figures were computed by MotherDuck on {formatTimestamp(
						storage.computed_at,
					)}.
				</p>
			{/if}
		</Panel>
	</div>

	<div class="mt-6">
		<Panel title="Query shapes">
			<ShapeTable
				{shapes}
				selected={shapeFilter}
				onSelect={toggleShape}
				emptyMessage={filtersActive
					? 'No query shapes match the current filters.'
					: 'No queries in this window yet.'}
			/>
		</Panel>
	</div>

	<div class="mt-6">
		<Panel title="What to review">
			<InsightList
				connectionId={connectionId ?? ''}
				{insights}
				selected={shapeFilter}
				onSelect={toggleShape}
				emptyMessage={filtersActive
					? 'Nothing to review in the queries that match these filters.'
					: 'Nothing to review in this window.'}
			/>
		</Panel>
	</div>

	<div class="mt-6">
		<Panel title="Slowest queries">
			<QueryTable
				events={slowQueries}
				emptyMessage={filtersActive
					? 'No queries match the current filters.'
					: 'No queries in this window yet.'}
				sort={slowSort.sort}
				dir={slowSort.dir}
				onSort={(key) => sortTable('slow', key)}
			/>
		</Panel>
	</div>

	<div class="mt-6">
		<Panel title="Recent failures">
			<QueryTable
				events={failures}
				emptyMessage={filtersActive
					? 'No failures match the current filters.'
					: 'No failures in this window.'}
				showError
				sort={failureSort.sort}
				dir={failureSort.dir}
				onSort={(key) => sortTable('failures', key)}
			/>
		</Panel>
	</div>
{/if}
