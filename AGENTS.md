# AGENTS.md

This file provides guidance to coding agents collaborating on this repository.

## Mission

DuckWatch is an observability tool for MotherDuck: a Rust backend (`backend/`) that ingests query history and storage
measurements from customer accounts, and a JavaScript (SvelteKit) frontend (`web/`) that reports latency, failures, and cost.
Priorities, in order:

1. Correctness of the numbers, since users make spending and reliability decisions from them.
2. Honesty about what a number means, since the cost figures are estimates rather than a MotherDuck bill.
3. Clean Architecture layering (domain, application, and infrastructure).
4. Minimalism and clear, maintainable code.

The repository began as a generic Rust web application template. Anything that still reads as generic scaffolding is
stale rather than deliberate.

## Core Rules

- Use simple and everyday English for code, comments, docs, and tests.
- Respect layer direction: `domain` depends on nothing, `application` depends on `domain`, and `infrastructure` depends
  on both. Do not import infrastructure types into domain or application code.
- Access external systems (PostgreSQL and MotherDuck) only through the trait interfaces in
  `backend/src/application/services/`. The MotherDuck client is a trait so that use cases stay testable without a token.
- Prefer small, focused changes over large refactoring.
- Add comments only when they clarify non-obvious behavior.
- Write SQL keywords in lowercase in migrations and queries: `create table`, not `CREATE TABLE`.
- Keep dependencies few. A new crate or npm package needs a reason that a hand-written alternative cannot meet, which
  is why the chart, the SQL highlighter, and the icons are hand-rolled.
- Money and time arithmetic belongs in `domain/`, never in SQL or in a component. The store counts and sums; the
  application prices.

Quick examples:

- Good: add a new service trait in `application/services/` and implement it in `infrastructure/`.
- Bad: call `sqlx` directly from a use case in `application/use_cases/`.

## Writing Style

- Write in simple, plain English. Use short sentences and everyday words.
- Use Oxford commas in inline lists: "a, b, and c" not "a, b, c".
- Do not use em dashes. Restructure the sentence, or use a colon or semicolon instead.
- Avoid colorful adjectives and adverbs. Write "adjacency query" not "blazing adjacency query".
- Prefer noun phrases for checklist items over imperative verbs. Write "temp directory teardown" not "tear down the temp directory".
- Headings in Markdown files must be in title case: "Build from Source" not "Build from source". Minor words (a, an, the, and, but, or, for, in, on,
  at, to, by, of) stay lowercase unless they are the first word.
- Do not bold the lead-in of a list item. Write "Vector and set similarity: ..." not "**Vector and set similarity**: ...".
- Use sentence case for the lead-in of a list item. Write "Seed selection: ..." not "Seed Selection: ...". Proper nouns keep their capitals.
- Capitalize only the first part of a hyphenated compound: "Full-text Search" in a heading, "Breadth-first" at the start of a sentence, and
  "breadth-first search" elsewhere. Never write "Breadth-First".
- Start each sentence with a capital letter, capitalize proper nouns (Rust, PostgreSQL, SvelteKit, TypeScript), and leave common nouns lowercase
  in the middle of a sentence.
- Write correct and complete sentences.
- Avoid made-up words, abbreviations, and colons in the middle of sentences.
- Use participial phrases scarcely.

## Architecture Constraints

- The Rust toolchain is pinned to 1.97.1 (edition 2024) by `rust-toolchain.toml`; do not add version suffixes to cargo
  commands.
- The frontend targets the active Node.js LTS line, which is 24. TypeScript stays on the 5.x line because
  `svelte-check` and `typescript-eslint` both reject TypeScript 7.
- The workspace denies `clippy::unwrap_used` and `clippy::expect_used` in production code. Test code is exempt through
  the `cfg_attr` at the top of `backend/src/main.rs`.
- Queries use the runtime `sqlx::query_as` API, not the `sqlx::query!` macros, so the project builds without a database
  connection and carries no `.sqlx` metadata directory. Keep it that way unless the whole project moves to the macros.
- Rows are mapped through a private `*Row` type in each `infrastructure/pg/` module, so no `sqlx` derive appears on a
  domain entity.
- `sqlx` 0.9 accepts only static query strings. A query assembled at runtime, such as the shared filter clause in
  `infrastructure/pg/query_events.rs`, must go through `AssertSqlSafe` and may interpolate only values from a fixed
  match, never caller input.
- The backend applies its migrations on startup. A schema change means a new migration pair, never an edit to an
  existing one.
- Field-level validation belongs in the `validator` rules on the request body; invariants belong in the domain
  constructor, such as `ConnectionDraft::new` or `Email::new`.
- The `duckdb` crate builds libduckdb from source through its `bundled` feature, so a cold backend build takes several
  minutes. The MotherDuck extension is downloaded and loaded at runtime, and the crate is blocking, so every call runs
  inside `spawn_blocking`.
- MotherDuck rates live in `backend/src/domain/entities/pricing.rs` with a link to the pricing page. Update them there
  when MotherDuck changes prices; no rate belongs anywhere else.
- DuckWatch's own polling must identify DuckWatch, never a schema or a table. Both signals it uses, the `-- duckwatch`
  marker comment and the user agent, are things only DuckWatch produces; a rule such as "mentions
  `md_information_schema`" also hides customers reading their own query history, which is a thing DuckWatch users do. A
  test asserts every statement the MotherDuck client sends carries the marker.
- Anti-pattern thresholds live in `backend/src/domain/entities/insights.rs`, each with the reason it was chosen. A
  finding is only raised once it accounts for enough of the period to be worth acting on, because roughly two thirds of
  real shapes carry at least one flag and an ungated list is noise. Detection is a heuristic. The interface does not say
  so in prose, and instead shows what each finding cost rather than asserting a saving. Every suggestion opens with
  "Try", so the copy stays a suggestion rather than a claim.
- Two poller passes catch up on data recorded before a feature existed: `backfill_fingerprints` and
  `backfill_antipatterns`, both bounded by `ingest_backfill_limit`. A shape examined and found clean stores an empty
  array rather than null, so it is not examined again. Either pass failing must never stall the sync.
- The ingestion watermark only ever moves forward. History is read oldest first under a row limit, so a batch cut short
  by that limit can end earlier than the watermark it started from, and storing that would walk the next fetch further
  back again. The overlap that catches late-published rows is skipped while a connection is catching up, because a
  full batch means the rows still owed sit past the watermark and reading the same window again would crowd them out
  for good. The history filter is inclusive of the instant it starts from, because a catching-up pass starts exactly at
  the watermark and rows can share that millisecond with it, so an exclusive filter would drop them and nothing would
  read them again.
- `last_synced_at` records the last attempt, whether or not it worked; `last_success_at` records the last success. Both
  are needed, or a connection failing for days is indistinguishable from one that just succeeded. A failed pass passes
  `None` for the success time, and the update coalesces so the stored one survives.
- Secrets are encrypted at rest with the key in `TOKEN_ENCRYPTION_KEY`. A MotherDuck token must never be serialized,
  logged, or included in an error message; `MotherDuckToken` redacts its own `Debug` output and the client scrubs the
  token out of driver errors.
- Configuration comes from environment variables parsed in `backend/src/config.rs`; `make run-backend` copies
  `backend/.env.example` to `backend/.env` when missing, and `make run-web` does the same for `web/.env.example`
  (`VITE_API_URL`, which only the development server needs; see the same-origin rule under Frontend Conventions).
- Never commit real secrets. The `.env` files are gitignored, and example values belong in the `.env.example` files.

## Product Facts Worth Knowing

These have each caused a wrong assumption at least once, and each is verified against MotherDuck's documentation.

- Reading `md_information_schema.query_history` needs a Business plan and the view query history permission. Reading
  `storage_info` needs the wider organization storage permission, so storage ingestion must fail on its own without
  taking query ingestion with it.
- A MotherDuck organization lives in exactly one cloud region, chosen when it is created, so one region tier per
  connection covers an account completely. Duckling sizes vary freely inside an account, and each query is priced at
  its own size's rate before any total is taken.
- DuckDB can parse MotherDuck SQL locally through `json_serialize_sql`, which is how query shapes are fingerprinted
  without a network call. It serializes SELECT statements only, so anything else takes the text fallback in
  `domain/entities/query_shapes.rs`.
- The same serialized tree carries the anti-pattern flags, verified against DuckDB's own output rather than assumed: a
  `select *` column is `class: "STAR"`, a comma join is `ref_type: "CROSS"` while its `join_type` still reads `INNER`,
  a missing filter is `where_clause: null`, and sorting and limiting appear as `ORDER_MODIFIER` and `LIMIT_MODIFIER`
  entries. Flags are read after literals are blanked, so they describe the shape rather than one run of it.
- MotherDuck reports the client's custom user agent in `query_history`, verified in August 2026 as
  `duckdb/v1.5.5(linux_amd64) rust duckwatch`, and it is already populated by the time a row is visible at all. The
  view publishes with a lag of its own: the soonest a query has been observed to appear is about two minutes after it
  ran, which sets the floor on how fresh the dashboard can be.
- A leading `-- duckwatch` comment survives the round trip through `query_history` intact, so DuckWatch marks its own
  statements with one. Either that marker or the user agent identifies its traffic, which means neither MotherDuck's
  agent behavior nor the shape of a statement has to be relied on alone.
- The query history view reports durations as intervals and has no rows read column. Ingestion converts intervals to
  milliseconds in SQL and reads timestamps as epoch milliseconds, which avoids any dependence on session time zone.
- MotherDuck refreshes `storage_info` every one to six hours, and its own documentation says so, while
  `storage_info_history` publishes one set of results a day however often the latest figures are recomputed. Reading
  storage on the query poll interval therefore bills the account for hundreds of identical reads a day, so ingestion
  reads it on `ingest_storage_interval_seconds` instead, defaulting to an hour. The attempt is recorded whether or not
  it worked, because a token without the storage permission fails every pass and is exactly the case worth backing off.
  That timer lives in the poller rather than a column, so a restart costs one extra read instead of a migration.
- Latency is elapsed time; cost is execution time. A reader waited the elapsed time, so the percentiles, the duration
  sort, the minimum duration filter, and the worst run column all read `total_elapsed_time_ms`. A Duckling only worked
  the execution time, and MotherDuck defines wait time as time a query spends queued while other queries hold the
  execution threads, so pricing reads `execution_time_ms` and falls back to elapsed only where that column is null.
  Pricing on elapsed time charged the waiting query for seconds the running one was already charged for, and put
  queries that were merely blocked near the top of the cost lists.
- The Pulse floor of one second applies to each run, not to a group total. A group is priced from the total, the run
  count, and how many runs came in under the floor along with the time they took, all of which the store counts and
  the tier turns into money. Flooring the total instead undercharges any group that mixes short runs with long ones,
  which by-user attribution does by construction.
- Compute estimates attribute Duckling time to individual queries. MotherDuck bills Standard and larger Ducklings for
  uptime instead, so concurrent queries share compute that DuckWatch charges to each of them, and idle Duckling time
  appears nowhere. Storage bills on average monthly usage, so it is reported as a monthly run rate rather than a charge
  for the selected range. The dashboard states neither point in prose. It carries them in labels instead, through the
  `Per month` column in the storage table, the `Est. cost` column in the shape table, and the `Estimated cost` chart
  title. Adding the prose back is a product decision rather than a fix.

## Frontend Conventions

- Colors come from the semantic tokens in `web/src/app.css`, such as `bg-surface`, `text-muted`, and `border-line`.
  Never write a raw palette class like `bg-white` or `text-gray-500`, because the dark theme is defined once by
  redefining those tokens.
- Every text token must clear a 4.5:1 contrast ratio against the surfaces it appears on, in both themes.
- The interface must not move when data changes. Tables use a fixed layout with explicit column widths, numbers use
  tabular figures, and a control whose label or presence depends on state gets a reserved slot instead of appearing and
  disappearing.
- Selecting a row must not move that row. Selecting a query shape or a finding refilters the panels above it, so those
  panels hold their height: a selection outline is drawn on a border that is always there and merely changes color, the
  stat tiles carry a height floor, and `AttributionTable` keeps space for the row count it had before the selection
  narrowed it. A conditional class that adds a border, padding, or a margin shifts the content beside it and needs a
  counterpart on the other branch.
- Text that distinguishes one row from another, such as a user name or an error type, must stay fully readable. A
  hover-only tooltip is not recovery for truncated text.
- Sections are framed with `Panel.svelte`. A component placed inside a panel must not draw its own frame.
- The API shares the page's origin. `web/src/lib/services/api/index.ts` falls back to a relative `/api/v1`, and
  `web/nginx.conf` proxies `/api/` to `backend:8080`, so the published image carries no backend URL and runs anywhere
  without a rebuild. Do not reintroduce a `VITE_API_URL` build argument in a Dockerfile or a compose file. It is a
  development-only override for the Vite server, which has no proxy of its own. The published image was once built
  without it and shipped `undefined` in place of the URL, which is the failure this arrangement removes.
- Timestamps are absolute instants everywhere. Render them through `formatTimestamp` in
  `web/src/lib/services/time.svelte.ts`, which names the zone and honors the local or UTC choice. The health banner is
  the one exception, and it reports ages relative to now, because a reader there wants the gap rather than the instant.
- Empty, loading, and error are three different states. An empty result must say whether there is no data or whether the
  filters excluded it, and a failed load must keep the last good data on screen with a retry. That covers the route
  loader as well. A `load` that lets an API failure escape replaces the whole page with the framework's error screen,
  which honors none of these rules, so `routes/+page.ts` catches the failure and hands the page a `loadFailed` flag to
  report itself. Each table says this for itself through its own empty message. The filter controls carry no notice of
  their own, because one that appears and disappears changes the height of the panel above every figure on the page.
- Ingestion state qualifies every number on the page, so the health banner sits above them and says what a stale or
  failing connection means for the figures below. A healthy connection still occupies the same slot, since a banner
  that appears and disappears moves the page.
- A capped list must say what the cap left out. The insights response carries the count and the per kind totals from
  before the cap for exactly this reason, and those totals overlap, so the interface must never add them together.
- Copy that describes a finding stays a suggestion. No component may claim a percentage saving, because the costs
  behind it are estimates.
- A button shows it was pressed with an inset bottom edge, which grows on hover and tightens on active, using
  `--color-accent-shade` or the matching token for its variant. An inset shadow costs no layout, so the press reads
  without moving anything beside it. Focus keeps a real outline rather than reusing that edge.
- Every field in `ListOptions` must be serialized by `params` in `web/src/lib/services/api/dashboard.ts`. A field the
  type declares and the builder skips leaves a filter that changes nothing on screen while the row still highlights,
  which is how the shape filter went unnoticed. A test in `web/test/dashboard-api.test.ts` covers the shape field.

## Required Validation

Run these checks for any non-trivial backend change:

1. `make test` (unit tests; no containers needed)
2. `make lint` (clippy denies warnings, `unwrap_used`, and `expect_used`)
3. `make format`

Run these as well when the change touches SQL, migrations, or the repository layer:

1. `make docker-up` (starts PostgreSQL)
2. `make test-integration` (unit and integration tests against the live container)

For frontend changes, run `make lint-web`, `make check-web`, and `make test-web`.

`make check-all` runs every check that does not need containers.

## Review Guidelines (P0/P1 Focus)

Review output should be concise and only include critical issues.

- `P0`: must-fix defects (security flaw, data loss, architecture breakage).
- `P1`: high-priority defects (likely functional bug, missing validation, layering violation).

Do not include:

- style-only nitpicks,
- praise/summary of what is already good,
- exhaustive restatement of the patch.

Use this review format:

1. `Severity` (`P0`/`P1`)
2. `File:line`
3. `Issue`
4. `Why it matters`
5. `Minimal fix direction`

## Practical Notes for Agents

- Prefer targeted edits over broad mechanical rewrites.
- If you detect contradictory repository conventions, follow existing code and update docs accordingly.
- The `tmp/` directory is gitignored scratch space. Do not read from it or write to it as part of a change, unless the
  user asks for it directly.
- Verify claims about MotherDuck against its documentation rather than from memory. Its behavior has already
  contradicted reasonable assumptions more than once.

## Commit and PR Hygiene

- Keep commits scoped to one logical change.
- Follow the existing conventional commit style: `feat(dashboard): ...`, `fix(ingestion): ...`, `docs(readme): ...`.
- PR descriptions should include:
    1. behavioral change summary,
    2. tests added/updated,
    3. migrations added (or "no schema change"),
    4. docs updated (yes/no).
