<div align="center">
  <picture>
    <img alt="DuckWatch Logo" src="docs/assets/logo.svg" width="120" height="120">
  </picture>
<br>

<h2>DuckWatch</h2>

[![Tests](https://img.shields.io/github/actions/workflow/status/CogitatorTech/duck-watch/tests.yml?label=tests&style=flat&labelColor=282c34&logo=github)](https://github.com/CogitatorTech/duck-watch/actions/workflows/tests.yml)
[![Code Coverage](https://img.shields.io/codecov/c/github/CogitatorTech/duck-watch?label=coverage&style=flat&labelColor=282c34&logo=codecov)](https://codecov.io/gh/CogitatorTech/duck-watch)
[![License](https://img.shields.io/badge/license-Apache--2.0-007ec6?style=flat&labelColor=282c34&logo=open-source-initiative)](LICENSE)
[![Docker](https://img.shields.io/badge/docker-ghcr.io-007ec6?style=flat&labelColor=282c34&logo=docker)](https://github.com/CogitatorTech/duck-watch/pkgs/container/duck-watch-backend)

An observability tool for MotherDuck

</div>

---

DuckWatch shows you what your MotherDuck account is doing and what it is costing you.

### Key Features

- Latency percentiles, failure counts, and the slowest queries over any time range
- Cost estimates per user and per Duckling size, next to the same length of time before
- Storage figures for every database, priced as a monthly rate
- Query shapes: runs that differ only in their values are counted as one
- Findings for patterns that waste money, such as `select *`, joins with no condition, unbounded sorts, and queries
  that run out of memory and spill to disk
- Ingestion health on every page, so figures that are behind say so instead of looking current
- History kept for as long as you want, past the 90 days MotherDuck retains

> [!IMPORTANT]
> The money figures are estimates, not a copy of your MotherDuck bill. DuckWatch attributes Duckling time to individual
> queries; MotherDuck bills Standard and larger Ducklings for uptime, so concurrent queries share compute that DuckWatch
> charges to each of them, and idle Duckling time appears nowhere. Storage is reported as a monthly run rate because
> MotherDuck bills on the average across a month. Use the numbers to compare queries against each other, not to predict
> an invoice.

> [!NOTE]
> This project is still in early development, so bugs and breaking changes are expected.
> Please use the [issue page](https://github.com/CogitatorTech/duck-watch/issues) to report bugs or request features.

---

### How It Works

1. A user signs up, which creates an organization, and adds a MotherDuck read-only service token on the connections
   page. The token is checked against MotherDuck before it is stored, and it is stored encrypted (AES-256-GCM).
2. A background poller in the backend syncs every enabled connection on an interval. It reads
   `md_information_schema.query_history` through a DuckDB connection, upserts the rows into PostgreSQL keyed by query
   id, and advances a per-connection watermark. Re-reading an overlap window is safe and picks up late-completing
   queries.
3. The dashboard reads aggregates (counts, p50/p95, per-instance-type attribution) scoped to the caller's organization.

> [!IMPORTANT]
> Reading `MD_INFORMATION_SCHEMA.QUERY_HISTORY` requires a MotherDuck Business plan and the "View query history"
> permission on the token's user. The connection test surfaces a clear error otherwise.

### Repository Layout

| Path                          | Contents                                                    |
|-------------------------------|-------------------------------------------------------------|
| `backend/`                    | Rust backend: Axum, sqlx, PostgreSQL, and the DuckDB client |
| `backend/src/domain/`         | Entities and domain errors, with no framework dependencies  |
| `backend/src/application/`    | Service traits and use cases                                |
| `backend/src/infrastructure/` | PostgreSQL, MotherDuck client, poller, HTTP routes          |
| `backend/migrations/`         | sqlx migrations as `.up.sql` and `.down.sql` pairs          |
| `web/`                        | SvelteKit frontend                                          |
| `docker-compose.yaml`         | `db`, `backend`, and `web` services                         |
| `Makefile`                    | Build, test, run, etc. commands                             |

---

### Running DuckWatch

You need Docker, and a MotherDuck service token from an account on the Business plan. Reading query history requires
that plan and the view query history permission; storage figures need the wider organization storage permission.

```bash
git clone https://github.com/CogitatorTech/duck-watch.git
cd duck-watch
make start
```

Then open <http://localhost:3000>, create an account, and add your MotherDuck token on the connections page. The first
poll runs within a minute, and MotherDuck publishes its query history with a delay of its own, so the first figures
appear a few minutes after that.

`make start` writes a `.env` file with a freshly generated `TOKEN_ENCRYPTION_KEY`, which is what encrypts your stored
MotherDuck token. Keep a backup of it alongside your database: without it, every stored token has to be entered again.

`make start` pulls prebuilt images from GHCR, so it takes about a minute rather than building anything. Use
`make stop` to stop DuckWatch and `make start` to bring it back with its data intact.
Follow what it is doing with `make logs`.

To build both images from your own checkout instead, use `make start-dev`.
That compiles libduckdb from C++ source and takes roughly twenty minutes the first time.

#### Container Images

Both images are published to GHCR for `linux/amd64` and `linux/arm64`:

```bash
docker pull ghcr.io/cogitatortech/duck-watch-backend:latest
docker pull ghcr.io/cogitatortech/duck-watch-web:latest
```

Compose pulls them for you, so you only need these if you are deploying DuckWatch somewhere other than your own
machine. The backend needs `DATABASE_URL` and `TOKEN_ENCRYPTION_KEY`, and applies its own migrations on startup.

---

### Developing DuckWatch

If you use Nix, `nix develop` (or `make shell`) gives you the pinned Rust toolchain, Node.js, sqlx-cli, and `psql` in
one shell. Otherwise install the tooling with `make install-deps`.

```bash
# Install the development tooling (sqlx-cli, cargo-audit, and the frontend packages)
make install-deps

# Start PostgreSQL
make docker-up

# Run the backend on http://localhost:8080
make run-backend

# In a second terminal, run the frontend on http://localhost:5173
make run-web
```

The backend applies its migrations on startup. The first backend build takes several minutes because the `duckdb`
crate compiles libduckdb from source.

`make run-backend` creates `backend/.env` from `backend/.env.example` when it is missing. Two settings deserve
attention:

- `TOKEN_ENCRYPTION_KEY`: a base64 encoded 32-byte key that encrypts stored MotherDuck tokens. Generate your own with
  `openssl rand -base64 32`. Losing the key orphans every stored token, so back it up together with the database.
- `INGEST_POLL_INTERVAL_SECONDS`, `INGEST_OVERLAP_MINUTES`, and `INGEST_BATCH_LIMIT`: the poller's cadence and batch
  bounds.

---

### API

`/health` reports liveness.
Everything else lives under `/api/v1` and, except for signup and login, requires an `Authorization: Bearer <token>` header.

| Method   | Path                            | Description                                        |
|----------|---------------------------------|----------------------------------------------------|
| `POST`   | `/api/v1/auth/signup`           | Create an organization and its first user          |
| `POST`   | `/api/v1/auth/login`            | Exchange email and password for a session token    |
| `POST`   | `/api/v1/auth/logout`           | End the session                                    |
| `GET`    | `/api/v1/me`                    | The caller's user and organization                 |
| `GET`    | `/api/v1/connections`           | List the org's MotherDuck connections              |
| `POST`   | `/api/v1/connections`           | Add a connection (the token is verified first)     |
| `DELETE` | `/api/v1/connections/{id}`      | Remove a connection and its ingested history       |
| `GET`    | `/api/v1/dashboard/summary`     | Counts, p50/p95, and per-instance-type attribution |
| `GET`    | `/api/v1/dashboard/latency`     | Latency chart buckets                              |
| `GET`    | `/api/v1/dashboard/slow-queries`| Slowest queries in the window                      |
| `GET`    | `/api/v1/dashboard/failures`    | Recent failed queries                              |
| `GET`    | `/api/v1/dashboard/event`       | One event with its full query text                 |
| `GET`    | `/api/v1/dashboard/attribution` | Cost per user and per Duckling size                |
| `GET`    | `/api/v1/dashboard/storage`     | Stored bytes per database and the monthly rate     |
| `GET`    | `/api/v1/dashboard/shapes`      | Query shapes ranked by cost                        |
| `GET`    | `/api/v1/admin/organizations`   | Cross-tenant overview, superadmin only             |

The dashboard endpoints take `connection_id` and either `window` (`1h`, `24h`, `7d`, or `30d`) or an explicit
`from`/`to` pair of RFC 3339 timestamps, which wins over the preset and may span at most 90 days. They also take `q` (substring search over the
query text), `user`, `type` (query category), and `min_ms` (minimum run time), which narrow the summary and the chart
as well as the lists, so every number on the dashboard describes the same set of queries. The lists additionally take
`limit`, `sort` (`started` or `duration`), and `dir` (`asc` or `desc`). `GET /api/v1/dashboard/filters` returns the distinct users and
categories in the window for filter menus. DuckWatch's own polling queries are tagged through a custom
DuckDB user agent, stored with an `is_internal` flag, and excluded everywhere unless `internal=true` is passed. The
event endpoint takes `connection_id` and `query_id`.

### Query Shapes

Ingestion fingerprints every statement, so runs that differ only in their literals count as one shape. A statement run
nightly appears once with its total cost rather than once per run. Fingerprints come from parsing the statement with
the DuckDB the backend already links, which speaks the same dialect as MotherDuck and runs locally, so no token and no
network are involved. DuckDB serializes SELECT statements only, so a statement such as `create table ... as select ...`
falls back to text normalization, which still removes comments, formatting, case, and literals. Queries stored before
this existed are fingerprinted a slice per poll, bounded by `INGEST_BACKFILL_LIMIT`.

Selecting a shape filters the whole dashboard to its runs, and `shape=<fingerprint>` does the same on the API.

### Cost Estimates

Each connection records the MotherDuck region it is billed in (US, Europe, or Asia Pacific). A MotherDuck
organization lives in exactly one region, chosen when it is created, so one region per connection covers an account
completely. The dashboard prices query time against the published Business plan rates per Duckling size, and a single
connection may mix Duckling sizes freely: each query is priced at its own size's rate before any total is taken. The
summary shows an estimated cost for the
range with a per-Duckling breakdown, every query carries its own estimate, and the attribution tables rank users and
Duckling sizes by spend, each with its share of the period and what the same group cost over the preceding period of
equal length. The latency chart also switches to plotting cost per bucket.

Storage is priced separately. The poller also reads `md_information_schema.storage_info` and records what each
database holds, which the dashboard prices at the region's per gigabyte-month rate. Reading that view needs a token
permitted to view organization storage, a wider permission than the query history requires; without it the storage
panel stays empty and query ingestion carries on unaffected.

> [!NOTE]
> The compute estimate is an attribution, not a bill. MotherDuck charges Standard and larger Ducklings for wall-clock uptime
> rather than per query, so concurrent queries share compute that DuckWatch attributes to each of them, and idle
> Duckling time appears nowhere. Pulse is billed per compute unit second with a one second floor, which the estimate
> applies per query. Rates come from
> [MotherDuck's pricing page](https://motherduck.com/docs/about-motherduck/billing/pricing/) and are hard-coded in
> `backend/src/domain/entities/pricing.rs`; update them there when MotherDuck changes them.

### Platform Superadmin

A superadmin is the platform operator: `GET /api/v1/admin/organizations` and the Admin page in the UI show every
organization with its user count and connection sync health. No signup or API path can grant the privilege; promote an
operator directly in the database:

```bash
make promote-admin EMAIL=you@example.com   # make demote-admin takes it away
```

The target runs the update inside the database container. Sign in again after the promotion so the session reflects
the flag.

The frontend keeps the session token in `localStorage` because the app ships as a static bundle without a server-side
cookie path. That is an accepted MVP tradeoff; a script injected through XSS could read the token.

### Development

```bash
make check-all         # Lints and tests for both the backend and the frontend
make test              # Backend unit tests, no containers needed
make test-integration  # Backend unit and integration tests, needs `make docker-up`
make format            # Format the Rust code
make lint              # Run clippy with warnings denied
```

A live test against a real MotherDuck account is ignored by default.
Run it by hand with a token:

```bash
MOTHERDUCK_TEST_TOKEN=... cargo test --workspace --features integration-tests -- --ignored
```

### Manual End-to-end Checklist

1. `make docker-up`, `make run-backend`, and `make run-web`.
2. Sign up with an organization name, an email, and a password.
3. Add a connection with a real MotherDuck service token; a bad token must be rejected with a clear message.
4. Wait one poll interval, then check the connections page shows a sync time and no error.
5. Open the dashboard and confirm the tiles, the latency chart, the slow queries, and any failures render.

### Roadmap

- Slack and Discord alerts for slow and failed queries, on per-organization alert rules.
- Anti-pattern detection from query plans (full table scans and Cartesian joins).
- Data freshness checks on target tables.

---

### Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for details on how to make a contribution.

### License

DuckWatch is licensed under the Apache License, Version 2.0 (see [LICENSE](LICENSE)).
