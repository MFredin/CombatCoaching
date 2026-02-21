/// SQLite session and pull storage.
///
/// Uses `rusqlite` with the `bundled` feature so SQLite is compiled in —
/// no system installation required.
///
/// WAL mode is enabled for better concurrent read performance (the UI
/// may query sessions while the engine is writing).
use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;

pub fn init(db_path: PathBuf) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(&db_path)?;

    conn.execute_batch("
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA synchronous   = NORMAL;

        CREATE TABLE IF NOT EXISTS sessions (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at  INTEGER NOT NULL,
            ended_at    INTEGER,
            player_name TEXT    NOT NULL DEFAULT '',
            player_guid TEXT    NOT NULL DEFAULT '',
            player_spec TEXT,
            realm       TEXT
        );

        CREATE TABLE IF NOT EXISTS pulls (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id  INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            pull_number INTEGER NOT NULL,
            started_at  INTEGER NOT NULL,
            ended_at    INTEGER,
            outcome     TEXT,
            encounter   TEXT
        );

        CREATE TABLE IF NOT EXISTS advice_events (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            pull_id    INTEGER NOT NULL REFERENCES pulls(id) ON DELETE CASCADE,
            fired_at   INTEGER NOT NULL,
            rule_key   TEXT    NOT NULL,
            severity   TEXT    NOT NULL,
            message    TEXT    NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_pulls_session    ON pulls(session_id);
        CREATE INDEX IF NOT EXISTS idx_advice_pull      ON advice_events(pull_id);
        CREATE INDEX IF NOT EXISTS idx_advice_rule      ON advice_events(rule_key);
    ")?;

    tracing::info!("SQLite initialised at {:?}", db_path);
    Ok(conn)
}
