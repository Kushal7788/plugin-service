use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};

use axum::extract::ws::WebSocket;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tlsn::{connection::ConnectionInfo, transcript::TranscriptCommitment};
use tokio::sync::{oneshot, Mutex};

use crate::config::SessionStoreConfig;
use crate::types::RevealConfig;

pub(crate) type LiveSocketMap = Arc<Mutex<HashMap<String, oneshot::Sender<WebSocket>>>>;
pub(crate) type SessionStoreRef = Arc<dyn SessionStore>;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: SessionStoreRef,
    pub(crate) live_socket_txs: LiveSocketMap,
    pub(crate) trusted_notary_keys: Arc<Vec<String>>,
    pub(crate) notary_signing_key: Arc<[u8; 32]>,
}

impl AppState {
    pub(crate) fn new(
        store_config: SessionStoreConfig,
        trusted_notary_keys: Vec<String>,
        notary_signing_key: [u8; 32],
    ) -> Result<Self, SessionStoreError> {
        let store: SessionStoreRef = match store_config {
            SessionStoreConfig::Memory => Arc::new(MemorySessionStore::default()),
            SessionStoreConfig::Sqlite { path } => Arc::new(SqliteSessionStore::open(path)?),
        };
        Ok(Self {
            store,
            live_socket_txs: Arc::new(Mutex::new(HashMap::new())),
            trusted_notary_keys: Arc::new(trusted_notary_keys),
            notary_signing_key: Arc::new(notary_signing_key),
        })
    }
}

pub(crate) trait SessionStore: Send + Sync {
    fn insert(&self, session_id: &str, record: &SessionRecord) -> Result<(), SessionStoreError>;
    fn get(&self, session_id: &str) -> Result<Option<SessionRecord>, SessionStoreError>;
    fn save(&self, session_id: &str, record: &SessionRecord) -> Result<(), SessionStoreError>;
    fn mark_stale_active_failed(
        &self,
        cutoff: DateTime<Utc>,
        message: &str,
    ) -> Result<usize, SessionStoreError>;
    fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<(), SessionStoreError>;
}

#[derive(Debug, Error)]
pub(crate) enum SessionStoreError {
    #[error("session store sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("session store serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("session store io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("session store lock poisoned")]
    LockPoisoned,
}

#[derive(Default)]
pub(crate) struct MemorySessionStore {
    sessions: StdMutex<HashMap<String, SessionRecord>>,
}

impl SessionStore for MemorySessionStore {
    fn insert(&self, session_id: &str, record: &SessionRecord) -> Result<(), SessionStoreError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        sessions.insert(session_id.to_string(), record.clone());
        Ok(())
    }

    fn get(&self, session_id: &str) -> Result<Option<SessionRecord>, SessionStoreError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        Ok(sessions.get(session_id).cloned())
    }

    fn save(&self, session_id: &str, record: &SessionRecord) -> Result<(), SessionStoreError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        sessions.insert(session_id.to_string(), record.clone());
        Ok(())
    }

    fn mark_stale_active_failed(
        &self,
        cutoff: DateTime<Utc>,
        message: &str,
    ) -> Result<usize, SessionStoreError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut updated = 0;
        for session in sessions.values_mut() {
            if session.created_at_utc < cutoff && session.status.is_active() {
                session.status = SessionStatus::Failed {
                    message: message.to_string(),
                };
                updated += 1;
            }
        }
        Ok(updated)
    }

    fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<(), SessionStoreError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        sessions.retain(|_, session| session.created_at_utc >= cutoff);
        Ok(())
    }
}

pub(crate) struct SqliteSessionStore {
    conn: StdMutex<Connection>,
}

impl SqliteSessionStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, SessionStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY NOT NULL,
                created_at_utc TEXT NOT NULL,
                updated_at_utc TEXT NOT NULL,
                record_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_created_at_utc
                ON sessions(created_at_utc);
            "#,
        )?;
        Ok(Self {
            conn: StdMutex::new(conn),
        })
    }

    fn serialize_record(record: &SessionRecord) -> Result<String, SessionStoreError> {
        Ok(serde_json::to_string(record)?)
    }

    fn deserialize_record(raw: String) -> Result<SessionRecord, SessionStoreError> {
        Ok(serde_json::from_str(&raw)?)
    }
}

impl SessionStore for SqliteSessionStore {
    fn insert(&self, session_id: &str, record: &SessionRecord) -> Result<(), SessionStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let record_json = Self::serialize_record(record)?;
        conn.execute(
            r#"
            INSERT INTO sessions (session_id, created_at_utc, updated_at_utc, record_json)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                session_id,
                record.created_at_utc.to_rfc3339(),
                Utc::now().to_rfc3339(),
                record_json,
            ],
        )?;
        Ok(())
    }

    fn get(&self, session_id: &str) -> Result<Option<SessionRecord>, SessionStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let raw = conn
            .query_row(
                "SELECT record_json FROM sessions WHERE session_id = ?1",
                params![session_id],
                |row| row.get::<_, String>(0),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        raw.map(Self::deserialize_record).transpose()
    }

    fn save(&self, session_id: &str, record: &SessionRecord) -> Result<(), SessionStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let record_json = Self::serialize_record(record)?;
        conn.execute(
            r#"
            INSERT INTO sessions (session_id, created_at_utc, updated_at_utc, record_json)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(session_id) DO UPDATE SET
                created_at_utc = excluded.created_at_utc,
                updated_at_utc = excluded.updated_at_utc,
                record_json = excluded.record_json
            "#,
            params![
                session_id,
                record.created_at_utc.to_rfc3339(),
                Utc::now().to_rfc3339(),
                record_json,
            ],
        )?;
        Ok(())
    }

    fn mark_stale_active_failed(
        &self,
        cutoff: DateTime<Utc>,
        message: &str,
    ) -> Result<usize, SessionStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let stale_rows = {
            let mut stmt = conn.prepare(
                "SELECT session_id, record_json FROM sessions WHERE created_at_utc < ?1",
            )?;
            let rows = stmt.query_map(params![cutoff.to_rfc3339()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut updated = 0;
        for (session_id, raw_record) in stale_rows {
            let mut record = Self::deserialize_record(raw_record)?;
            if !record.status.is_active() {
                continue;
            }

            record.status = SessionStatus::Failed {
                message: message.to_string(),
            };
            let record_json = Self::serialize_record(&record)?;
            conn.execute(
                r#"
                UPDATE sessions
                SET updated_at_utc = ?2,
                    record_json = ?3
                WHERE session_id = ?1
                "#,
                params![session_id, Utc::now().to_rfc3339(), record_json],
            )?;
            updated += 1;
        }

        Ok(updated)
    }

    fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<(), SessionStoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM sessions WHERE created_at_utc < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionRecord {
    pub(crate) client_session_id: String,
    pub(crate) max_recv_data: usize,
    pub(crate) max_sent_data: usize,
    pub(crate) created_at_utc: DateTime<Utc>,
    pub(crate) session_data: HashMap<String, String>,
    pub(crate) reveal_policy: SessionRevealPolicy,
    pub(crate) reveal_config: Option<RevealConfig>,
    pub(crate) notary_material: Option<NotaryMaterial>,
    pub(crate) status: SessionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NotaryMaterial {
    pub(crate) connection_info: ConnectionInfo,
    pub(crate) server_ephemeral_key: tlsn::connection::ServerEphemKey,
    pub(crate) transcript_commitments: Vec<TranscriptCommitment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum SessionStatus {
    Pending,
    InProgress,
    Completed(SessionResult),
    Failed { message: String },
}

impl SessionStatus {
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionResult {
    pub(crate) server_name: String,
    pub(crate) transcript_sent_len: usize,
    pub(crate) transcript_recv_len: usize,
    pub(crate) transcript_sent_digest_base64: String,
    pub(crate) transcript_recv_digest_base64: String,
    pub(crate) verified_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionRevealPolicy {
    pub(crate) allowed_request_headers: Vec<String>,
    pub(crate) allowed_response_body_json_paths: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> SessionRecord {
        SessionRecord {
            client_session_id: "client-1".to_string(),
            max_recv_data: 1024,
            max_sent_data: 2048,
            created_at_utc: Utc::now(),
            session_data: HashMap::new(),
            reveal_policy: SessionRevealPolicy {
                allowed_request_headers: vec!["authorization".to_string()],
                allowed_response_body_json_paths: vec!["$.ok".to_string()],
            },
            reveal_config: None,
            notary_material: None,
            status: SessionStatus::Pending,
        }
    }

    fn stale_record(status: SessionStatus) -> SessionRecord {
        SessionRecord {
            created_at_utc: Utc::now() - chrono::Duration::minutes(20),
            status,
            ..record()
        }
    }

    #[test]
    fn memory_store_round_trips_session() {
        let store = MemorySessionStore::default();
        store
            .insert("s1", &record())
            .expect("insert should succeed");
        let loaded = store.get("s1").expect("get should succeed").unwrap();
        assert_eq!(loaded.client_session_id, "client-1");
    }

    #[test]
    fn memory_store_marks_stale_active_sessions_failed() {
        let store = MemorySessionStore::default();
        store
            .insert("pending", &stale_record(SessionStatus::Pending))
            .expect("insert should succeed");
        store
            .insert("fresh", &record())
            .expect("insert should succeed");

        let count = store
            .mark_stale_active_failed(Utc::now() - chrono::Duration::minutes(10), "expired")
            .expect("mark should succeed");

        assert_eq!(count, 1);
        let stale = store.get("pending").expect("get should succeed").unwrap();
        assert!(matches!(stale.status, SessionStatus::Failed { .. }));
        let fresh = store.get("fresh").expect("get should succeed").unwrap();
        assert!(matches!(fresh.status, SessionStatus::Pending));
    }

    #[test]
    fn sqlite_store_round_trips_session() {
        let path = std::env::temp_dir().join(format!(
            "mobile-tlsn-session-store-test-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let store = SqliteSessionStore::open(&path).expect("open should succeed");
        store
            .insert("s1", &record())
            .expect("insert should succeed");
        let loaded = store.get("s1").expect("get should succeed").unwrap();
        assert_eq!(loaded.max_sent_data, 2048);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sqlite_store_marks_stale_active_sessions_failed() {
        let path = std::env::temp_dir().join(format!(
            "mobile-tlsn-session-store-test-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let store = SqliteSessionStore::open(&path).expect("open should succeed");
        store
            .insert("in-progress", &stale_record(SessionStatus::InProgress))
            .expect("insert should succeed");
        store
            .insert("fresh", &record())
            .expect("insert should succeed");

        let count = store
            .mark_stale_active_failed(Utc::now() - chrono::Duration::minutes(10), "expired")
            .expect("mark should succeed");

        assert_eq!(count, 1);
        let stale = store
            .get("in-progress")
            .expect("get should succeed")
            .unwrap();
        assert!(matches!(stale.status, SessionStatus::Failed { .. }));
        let fresh = store.get("fresh").expect("get should succeed").unwrap();
        assert!(matches!(fresh.status, SessionStatus::Pending));
        let _ = std::fs::remove_file(path);
    }
}
