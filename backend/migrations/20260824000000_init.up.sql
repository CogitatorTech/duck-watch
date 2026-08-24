-- DuckWatch is a single account tool: one team runs one instance against one
-- MotherDuck account. The first person to open a fresh install claims it, and
-- the insert that does so refuses a second row.
create table if not exists users (
    id uuid primary key,
    email varchar(320) not null unique,
    -- argon2id
    password_hash text not null,
    created_at timestamp with time zone not null,
    updated_at timestamp with time zone not null
);

-- Opaque bearer tokens, stored only as a sha-256 hash so the database never holds anything that could be replayed.
create table if not exists sessions (
    id uuid primary key,
    user_id uuid not null references users (id) on delete cascade,
    token_hash bytea not null unique,
    expires_at timestamp with time zone not null,
    created_at timestamp with time zone not null
);

create index if not exists idx_sessions_user_id on sessions (user_id);

create table if not exists motherduck_connections (
    id uuid primary key,
    name varchar(128) not null,
    -- aes-256-gcm ciphertext of the motherduck service token; the key comes from the TOKEN_ENCRYPTION_KEY
    -- environment variable
    token_ciphertext bytea not null,
    token_nonce bytea not null,
    -- Which MotherDuck price tier the account's region falls in. An organization lives in exactly one region,
    -- so one tier covers it.
    region_tier varchar(16) not null default 'tier1',
    enabled boolean not null default true,
    -- ingestion watermark: the max start_time ingested so far (null before the first successful sync)
    watermark_start_time timestamp with time zone,
    -- When the poller last tried, whether or not it worked.
    last_synced_at timestamp with time zone,
    -- When the poller last succeeded. Separate from the attempt above, or a
    -- connection failing for days looks the same as one that just worked.
    last_success_at timestamp with time zone,
    last_sync_error text,
    created_at timestamp with time zone not null,
    updated_at timestamp with time zone not null
);

create table if not exists query_events (
    connection_id uuid not null references motherduck_connections (id) on delete cascade,
    md_query_id uuid not null,
    query_text text not null,
    query_type varchar(32),
    start_time timestamp with time zone not null,
    end_time timestamp with time zone,
    execution_time_ms bigint,
    wait_time_ms bigint,
    total_elapsed_time_ms bigint,
    error_type varchar(128),
    error_message text,
    user_name varchar(256),
    instance_type varchar(32),
    duckling_id varchar(256),
    session_name varchar(256),
    bytes_uploaded bigint,
    bytes_downloaded bigint,
    bytes_spilled_to_disk bigint,
    -- The client MotherDuck reports. Stored so that the internal flag below can be checked rather than trusted.
    user_agent varchar(256),
    -- DuckWatch's own polling traffic, hidden on the dashboard by default.
    is_internal boolean not null default false,
    -- Assigned once the statement has been analyzed, so rows read before that step ran carry null.
    fingerprint varchar(16),
    ingested_at timestamp with time zone not null,
    primary key (connection_id, md_query_id)
);

-- Dashboard reads scan a time window per connection.
create index if not exists idx_query_events_conn_start
    on query_events (connection_id, start_time desc);

-- Recent failures are a small subset, so a partial index keeps it cheap.
create index if not exists idx_query_events_failures
    on query_events (connection_id, start_time desc)
    where error_type is not null;

create index if not exists idx_query_events_fingerprint
    on query_events (connection_id, fingerprint);

-- Rows still waiting to be fingerprinted, which the poller works through.
create index if not exists idx_query_events_unfingerprinted
    on query_events (connection_id)
    where fingerprint is null;

-- The normalized text is stored once per shape rather than once per run.
create table if not exists query_shapes (
    connection_id uuid not null references motherduck_connections (id) on delete cascade,
    fingerprint varchar(16) not null,
    normalized_sql text not null,
    -- a real statement from the family, so the interface shows something a person recognizes
    example_sql text not null,
    -- false when the statement could not be parsed and the text fallback ran
    parsed boolean not null,
    -- Anti-pattern flags, named by the domain's `Antipattern` enum. Null means the shape has not been examined yet,
    -- which is what the backfill looks for; an empty array means examined and clean.
    antipatterns varchar(32) [],
    first_seen timestamp with time zone not null,
    primary key (connection_id, fingerprint)
);

create index if not exists idx_query_shapes_unflagged
    on query_shapes (connection_id)
    where antipatterns is null;

-- MotherDuck computes storage periodically rather than continuously, so a sample is keyed by the time it was computed.
create table if not exists storage_samples (
    connection_id uuid not null references motherduck_connections (id) on delete cascade,
    database_name varchar(256) not null,
    computed_at timestamp with time zone not null,
    active_bytes bigint not null,
    historical_bytes bigint not null,
    retained_for_clone_bytes bigint not null,
    failsafe_bytes bigint not null,
    ingested_at timestamp with time zone not null,
    primary key (connection_id, database_name, computed_at)
);

create index if not exists idx_storage_samples_latest
    on storage_samples (connection_id, computed_at desc);
