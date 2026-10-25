-- Migration 2 for crates/swiftdm-core/src/store.rs (SQLite). Forward-only.
-- Times are REAL unix seconds (UTC), like migration 1. Booleans are INTEGER 0/1.

-- Site grabber projects. Settings that are only read as a whole live in `config` (JSON),
-- the same way queues are stored; columns are the ones the UI lists and sorts by.
CREATE TABLE grabber_projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    start_url TEXT NOT NULL,
    template TEXT NOT NULL DEFAULT 'custom'
        CHECK (template IN ('images','audio','video','files','site','custom')),
    config TEXT NOT NULL,            -- save mode, explore rules, filters, login ref, schedule
    status TEXT NOT NULL DEFAULT 'idle'
        CHECK (status IN ('idle','exploring','downloading','stopped','done','error')),
    created_at REAL NOT NULL,
    updated_at REAL NOT NULL,
    last_run_at REAL
);
CREATE UNIQUE INDEX grabber_projects_name ON grabber_projects(lower(name));

-- Every page and file the crawler found. Pages are explored; files are offered.
CREATE TABLE grabber_items (
    id INTEGER PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES grabber_projects(id) ON DELETE CASCADE,
    url TEXT NOT NULL,               -- normalized (no fragment, sorted query)
    parent_url TEXT,
    depth INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('page','file')),
    content_type TEXT,
    size INTEGER,
    link_text TEXT,
    checked INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'found'
        CHECK (status IN ('found','explored','skipped','failed','queued','downloaded')),
    -- a job created from this item; the job can be removed without losing the item
    job_id TEXT REFERENCES jobs(id) ON DELETE SET NULL,
    found_at REAL NOT NULL,
    UNIQUE (project_id, url)
);
CREATE INDEX grabber_items_project_kind ON grabber_items(project_id, kind, status);
CREATE INDEX grabber_items_job ON grabber_items(job_id);

-- What the server said about a file in a synchronization queue at its last check.
CREATE TABLE sync_checks (
    job_id TEXT PRIMARY KEY REFERENCES jobs(id) ON DELETE CASCADE,
    checked_at REAL NOT NULL,
    size INTEGER,
    etag TEXT,
    last_modified TEXT,
    result TEXT NOT NULL CHECK (result IN ('unchanged','changed','gone','error'))
);

UPDATE schema_version SET v = 2;
