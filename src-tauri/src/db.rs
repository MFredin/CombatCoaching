/// SQLite session and pull storage.
///
/// Uses `rusqlite` with the `bundled` feature so SQLite is compiled in —
/// no system installation required.
///
/// The writer runs on a dedicated `std::thread` (rusqlite::Connection is !Send
/// across await points) and receives commands via a bounded sync channel.
/// Callers hold a cheap `DbWriter` handle that is Clone + Send + Sync.
///
/// Read queries (e.g. pull history) open their own short-lived read-only
/// connection from a Tauri command handler via `spawn_blocking`, keeping the
/// writer thread focused on writes only.
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Commands sent to the writer thread
// ---------------------------------------------------------------------------

pub enum DbCommand {
    InsertSession {
        reply:       oneshot::Sender<Result<i64>>,
        started_at:  u64,
        player_name: String,
        player_guid: String,
    },
    UpdateSession {
        session_id:  i64,
        player_name: String,
        player_guid: String,
    },
    InsertPull {
        reply:       oneshot::Sender<Result<i64>>,
        session_id:  i64,
        pull_number: u32,
        started_at:  u64,
    },
    EndPull {
        pull_id:  i64,
        ended_at: u64,
        outcome:  String,
    },
    InsertAdvice {
        pull_id:  i64,
        fired_at: u64,
        rule_key: String,
        severity: String,
        message:  String,
    },
    Shutdown,
}

// ---------------------------------------------------------------------------
// DbWriter — cheap handle, Clone + Send + Sync
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct DbWriter {
    tx: std::sync::mpsc::SyncSender<DbCommand>,
}

impl DbWriter {
    /// Insert a new session row; returns the auto-generated row id.
    pub async fn insert_session(
        &self,
        started_at:  u64,
        player_name: String,
        player_guid: String,
    ) -> Result<i64> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(DbCommand::InsertSession { reply: reply_tx, started_at, player_name, player_guid })
            .map_err(|_| anyhow::anyhow!("DB writer channel closed"))?;
        reply_rx.await.map_err(|_| anyhow::anyhow!("DB reply channel closed"))?
    }

    /// Back-fill player identity into the session row (fire-and-forget).
    pub fn update_session(&self, session_id: i64, player_name: String, player_guid: String) {
        let _ = self.tx.send(DbCommand::UpdateSession { session_id, player_name, player_guid });
    }

    /// Insert a new pull row; returns the auto-generated row id.
    pub async fn insert_pull(
        &self,
        session_id:  i64,
        pull_number: u32,
        started_at:  u64,
    ) -> Result<i64> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(DbCommand::InsertPull { reply: reply_tx, session_id, pull_number, started_at })
            .map_err(|_| anyhow::anyhow!("DB writer channel closed"))?;
        reply_rx.await.map_err(|_| anyhow::anyhow!("DB reply channel closed"))?
    }

    /// Update a pull's end time and outcome (fire-and-forget).
    pub fn end_pull(&self, pull_id: i64, ended_at: u64, outcome: String) {
        let _ = self.tx.send(DbCommand::EndPull { pull_id, ended_at, outcome });
    }

    /// Insert an advice event (fire-and-forget).
    pub fn insert_advice(
        &self,
        pull_id:  i64,
        fired_at: u64,
        rule_key: String,
        severity: String,
        message:  String,
    ) {
        let _ = self.tx.send(DbCommand::InsertAdvice { pull_id, fired_at, rule_key, severity, message });
    }
}

// ---------------------------------------------------------------------------
// spawn_db_writer — initialises SQLite and starts the writer thread
// ---------------------------------------------------------------------------

/// Initialise SQLite at `db_path`, apply the schema, and spawn the writer
/// thread. Returns a `DbWriter` handle that can be cloned freely.
pub fn spawn_db_writer(db_path: &Path) -> Result<DbWriter> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(db_path)?;
    apply_schema(&conn)?;

    let (tx, rx) = std::sync::mpsc::sync_channel::<DbCommand>(512);

    std::thread::spawn(move || db_writer_loop(rx, conn));

    tracing::info!("SQLite writer started at {:?}", db_path);
    Ok(DbWriter { tx })
}

fn apply_schema(conn: &Connection) -> Result<()> {
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

        CREATE INDEX IF NOT EXISTS idx_pulls_session ON pulls(session_id);
        CREATE INDEX IF NOT EXISTS idx_advice_pull   ON advice_events(pull_id);
        CREATE INDEX IF NOT EXISTS idx_advice_rule   ON advice_events(rule_key);
    ")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Writer loop (runs on its own std::thread)
// ---------------------------------------------------------------------------

fn db_writer_loop(rx: std::sync::mpsc::Receiver<DbCommand>, conn: Connection) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            DbCommand::InsertSession { reply, started_at, player_name, player_guid } => {
                let result = conn
                    .execute(
                        "INSERT INTO sessions (started_at, player_name, player_guid) VALUES (?1, ?2, ?3)",
                        params![started_at, player_name, player_guid],
                    )
                    .map(|_| conn.last_insert_rowid())
                    .map_err(anyhow::Error::from);
                let _ = reply.send(result);
            }

            DbCommand::UpdateSession { session_id, player_name, player_guid } => {
                if let Err(e) = conn.execute(
                    "UPDATE sessions SET player_name = ?1, player_guid = ?2 WHERE id = ?3",
                    params![player_name, player_guid, session_id],
                ) {
                    tracing::warn!("DB update_session error: {}", e);
                }
            }

            DbCommand::InsertPull { reply, session_id, pull_number, started_at } => {
                let result = conn
                    .execute(
                        "INSERT INTO pulls (session_id, pull_number, started_at) VALUES (?1, ?2, ?3)",
                        params![session_id, pull_number, started_at],
                    )
                    .map(|_| conn.last_insert_rowid())
                    .map_err(anyhow::Error::from);
                let _ = reply.send(result);
            }

            DbCommand::EndPull { pull_id, ended_at, outcome } => {
                if let Err(e) = conn.execute(
                    "UPDATE pulls SET ended_at = ?1, outcome = ?2 WHERE id = ?3",
                    params![ended_at, outcome, pull_id],
                ) {
                    tracing::warn!("DB end_pull error: {}", e);
                }
            }

            DbCommand::InsertAdvice { pull_id, fired_at, rule_key, severity, message } => {
                if let Err(e) = conn.execute(
                    "INSERT INTO advice_events (pull_id, fired_at, rule_key, severity, message) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![pull_id, fired_at, rule_key, severity, message],
                ) {
                    tracing::warn!("DB insert_advice error: {}", e);
                }
            }

            DbCommand::Shutdown => break,
        }
    }
}
