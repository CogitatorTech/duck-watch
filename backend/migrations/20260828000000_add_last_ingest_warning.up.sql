-- A note from a sync that worked but lost rows for good. Separate from
-- last_sync_error, because an error means syncing is broken and a warning
-- means the figures are missing something while syncing goes on working.
alter table motherduck_connections
    add column last_ingest_warning text;
