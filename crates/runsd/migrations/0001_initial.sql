-- WAL mode and synchronous=NORMAL are set programmatically (cannot run inside a transaction).
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS runs (
    id              TEXT PRIMARY KEY,
    jira_issue_id   TEXT NOT NULL,
    submitted_by    TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_runs_jira    ON runs(jira_issue_id);
CREATE INDEX IF NOT EXISTS idx_runs_created ON runs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_status  ON runs(status);

CREATE TABLE IF NOT EXISTS calculations (
    id               TEXT PRIMARY KEY,
    run_id           TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL,
    input_json       TEXT NOT NULL,
    idempotency_key  TEXT NOT NULL,
    status           TEXT NOT NULL CHECK (status IN
        ('pending','running','retrying','succeeded','failed','cancelled')),
    attempt          INTEGER NOT NULL DEFAULT 0,
    max_attempts     INTEGER NOT NULL DEFAULT 5,
    next_attempt_at  INTEGER,
    lease_owner      TEXT,
    lease_expires_at INTEGER,
    error_kind       TEXT,
    error_message    TEXT,
    result_path      TEXT,
    created_at       INTEGER NOT NULL,
    started_at       INTEGER,
    completed_at     INTEGER,
    updated_at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_calc_run          ON calculations(run_id);
CREATE INDEX IF NOT EXISTS idx_calc_status       ON calculations(status);
CREATE INDEX IF NOT EXISTS idx_calc_lease        ON calculations(lease_expires_at) WHERE status = 'running';
CREATE INDEX IF NOT EXISTS idx_calc_next_attempt ON calculations(next_attempt_at)  WHERE status = 'retrying';

CREATE TABLE IF NOT EXISTS events (
    seq             INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id          TEXT,
    calculation_id  TEXT,
    kind            TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_run ON events(run_id, seq);
