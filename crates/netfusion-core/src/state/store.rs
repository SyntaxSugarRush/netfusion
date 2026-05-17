// SPDX-License-Identifier: MIT OR Apache-2.0

//! SQLite-backed state persistence for crash recovery.
//!
//! Stores:
//! - Bond state (active members, failover state)
//! - Event log (structured events with timestamps)
//! - Health score history (rolling window per interface)
//! - Applied configuration snapshot

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;
use tracing::{debug, info, warn};

/// State store errors.
#[derive(Debug, Error)]
pub enum StateStoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("state file corrupted: {0}")]
    Corrupted(String),

    #[error("migration failed: {0}")]
    Migration(String),
}

/// Result alias.
pub type StateResult<T> = Result<T, StateStoreError>;

/// Stored bond state.
#[derive(Debug, Clone)]
pub struct StoredBondState {
    pub name: String,
    pub active_members: String, // JSON array
    pub standby_members: String, // JSON array
    pub failover_active: bool,
    pub last_failover: Option<DateTime<Utc>>,
    pub bond_interface: Option<String>,
}

/// Stored event entry.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub id: i64,
    pub event_type: String,
    pub data: String, // JSON
    pub timestamp: DateTime<Utc>,
}

/// Stored health score entry.
#[derive(Debug, Clone)]
pub struct StoredHealthEntry {
    pub id: i64,
    pub interface: String,
    pub overall: f64,
    pub rtt: f64,
    pub jitter: f64,
    pub loss: f64,
    pub throughput: f64,
    pub stability: f64,
    pub timestamp: DateTime<Utc>,
}

/// The state store manages persistent state via SQLite.
pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    /// Open or create the state database.
    pub fn open(path: &str) -> StateResult<Self> {
        // Create parent directory if needed
        if let Some(parent) = Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(path).map_err(StateStoreError::Db)?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )
        .map_err(StateStoreError::Db)?;

        let store = Self { conn };
        store.migrate()?;

        info!("State store opened at {}", path);
        Ok(store)
    }

    /// Open an in-memory database (for testing).
    #[cfg(test)]
    pub fn in_memory() -> StateResult<Self> {
        let conn = Connection::open_in_memory().map_err(StateStoreError::Db)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Run database migrations.
    fn migrate(&self) -> StateResult<()> {
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS bond_state (
                    name TEXT PRIMARY KEY,
                    active_members TEXT NOT NULL DEFAULT '[]',
                    standby_members TEXT NOT NULL DEFAULT '[]',
                    failover_active INTEGER NOT NULL DEFAULT 0,
                    last_failover TEXT,
                    bond_interface TEXT
                );

                CREATE TABLE IF NOT EXISTS events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_type TEXT NOT NULL,
                    data TEXT NOT NULL,
                    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
                CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);

                CREATE TABLE IF NOT EXISTS health_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    interface TEXT NOT NULL,
                    overall REAL NOT NULL,
                    rtt REAL NOT NULL,
                    jitter REAL NOT NULL,
                    loss REAL NOT NULL,
                    throughput REAL NOT NULL,
                    stability REAL NOT NULL,
                    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE INDEX IF NOT EXISTS idx_health_interface ON health_history(interface);
                CREATE INDEX IF NOT EXISTS idx_health_timestamp ON health_history(timestamp);

                CREATE TABLE IF NOT EXISTS config_snapshot (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    config TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                ",
            )
            .map_err(StateStoreError::Db)?;

        debug!("State store migrations complete");
        Ok(())
    }

    // === Bond State ===

    /// Save bond state.
    pub fn save_bond_state(&self, state: &StoredBondState) -> StateResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO bond_state
             (name, active_members, standby_members, failover_active, last_failover, bond_interface)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                state.name,
                state.active_members,
                state.standby_members,
                state.failover_active as i64,
                state.last_failover.map(|t| t.to_rfc3339()),
                state.bond_interface,
            ],
        )?;
        Ok(())
    }

    /// Load bond state by name.
    pub fn load_bond_state(&self, name: &str) -> StateResult<Option<StoredBondState>> {
        let row = self.conn.query_row(
            "SELECT name, active_members, standby_members, failover_active, last_failover, bond_interface
             FROM bond_state WHERE name = ?1",
            params![name],
            |row| {
                let failover_active: i64 = row.get(3)?;
                let last_failover: Option<String> = row.get(4)?;
                Ok(StoredBondState {
                    name: row.get(0)?,
                    active_members: row.get(1)?,
                    standby_members: row.get(2)?,
                    failover_active: failover_active != 0,
                    last_failover: last_failover.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
                    bond_interface: row.get(5)?,
                })
            },
        ).optional()?;
        Ok(row)
    }

    /// Load all bond states.
    pub fn load_all_bond_states(&self) -> StateResult<Vec<StoredBondState>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, active_members, standby_members, failover_active, last_failover, bond_interface
             FROM bond_state",
        )?;

        let states = stmt
            .query_map([], |row| {
                let failover_active: i64 = row.get(3)?;
                let last_failover: Option<String> = row.get(4)?;
                Ok(StoredBondState {
                    name: row.get(0)?,
                    active_members: row.get(1)?,
                    standby_members: row.get(2)?,
                    failover_active: failover_active != 0,
                    last_failover: last_failover.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
                    bond_interface: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(states)
    }

    /// Delete bond state.
    pub fn delete_bond_state(&self, name: &str) -> StateResult<()> {
        self.conn.execute(
            "DELETE FROM bond_state WHERE name = ?1",
            params![name],
        )?;
        Ok(())
    }

    // === Events ===

    /// Append an event to the log.
    pub fn append_event(&self, event_type: &str, data: &str) -> StateResult<i64> {
        let id = self.conn.execute(
            "INSERT INTO events (event_type, data, timestamp) VALUES (?1, ?2, ?3)",
            params![event_type, data, Utc::now().to_rfc3339()],
        )? as i64;
        Ok(id)
    }

    /// Get recent events (most recent N).
    pub fn get_recent_events(&self, limit: usize) -> StateResult<Vec<StoredEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_type, data, timestamp FROM events ORDER BY timestamp DESC LIMIT ?1",
        )?;

        let events = stmt
            .query_map(params![limit as i64], |row| {
                let timestamp: String = row.get(3)?;
                Ok(StoredEvent {
                    id: row.get(0)?,
                    event_type: row.get(1)?,
                    data: row.get(2)?,
                    timestamp: DateTime::parse_from_rfc3339(&timestamp)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    /// Trim old events beyond the retention window.
    pub fn trim_events(&self, max_count: usize) -> StateResult<usize> {
        // Delete events beyond the max_count, oldest first
        let deleted = self.conn.execute(
            "DELETE FROM events WHERE id IN (
                SELECT id FROM events ORDER BY timestamp ASC
                LIMIT MAX(0, (SELECT COUNT(*) FROM events) - ?1)
            )",
            params![max_count as i64],
        )?;
        Ok(deleted)
    }

    // === Health History ===

    /// Record a health score entry.
    pub fn record_health(&self, entry: &StoredHealthEntry) -> StateResult<i64> {
        let id = self.conn.execute(
            "INSERT INTO health_history
             (interface, overall, rtt, jitter, loss, throughput, stability, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.interface,
                entry.overall,
                entry.rtt,
                entry.jitter,
                entry.loss,
                entry.throughput,
                entry.stability,
                entry.timestamp.to_rfc3339(),
            ],
        )? as i64;
        Ok(id)
    }

    /// Get health history for an interface within a time window.
    pub fn get_health_history(
        &self,
        interface: &str,
        max_entries: usize,
    ) -> StateResult<Vec<StoredHealthEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, interface, overall, rtt, jitter, loss, throughput, stability, timestamp
             FROM health_history
             WHERE interface = ?1
             ORDER BY timestamp DESC
             LIMIT ?2",
        )?;

        let entries = stmt
            .query_map(params![interface, max_entries as i64], |row| {
                let timestamp: String = row.get(8)?;
                Ok(StoredHealthEntry {
                    id: row.get(0)?,
                    interface: row.get(1)?,
                    overall: row.get(2)?,
                    rtt: row.get(3)?,
                    jitter: row.get(4)?,
                    loss: row.get(5)?,
                    throughput: row.get(6)?,
                    stability: row.get(7)?,
                    timestamp: DateTime::parse_from_rfc3339(&timestamp)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    // === Config Snapshot ===

    /// Save the current configuration snapshot.
    pub fn save_config_snapshot(&self, config_json: &str) -> StateResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO config_snapshot (id, config, applied_at)
             VALUES (1, ?1, ?2)",
            params![config_json, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Load the last saved configuration snapshot.
    pub fn load_config_snapshot(&self) -> StateResult<Option<String>> {
        let config: Option<String> = self
            .conn
            .query_row(
                "SELECT config FROM config_snapshot WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bond_state_crud() {
        let store = StateStore::in_memory().unwrap();

        let state = StoredBondState {
            name: "test_bond".into(),
            active_members: "[\"eth0\"]".into(),
            standby_members: "[\"eth1\"]".into(),
            failover_active: false,
            last_failover: None,
            bond_interface: Some("netfusion0".into()),
        };

        store.save_bond_state(&state).unwrap();

        let loaded = store.load_bond_state("test_bond").unwrap().unwrap();
        assert_eq!(loaded.name, "test_bond");
        assert_eq!(loaded.active_members, "[\"eth0\"]");
        assert_eq!(loaded.bond_interface, Some("netfusion0".into()));

        store.delete_bond_state("test_bond").unwrap();
        assert!(store.load_bond_state("test_bond").unwrap().is_none());
    }

    #[test]
    fn test_event_log() {
        let store = StateStore::in_memory().unwrap();

        store.append_event("interface_up", r#"{"interface": "eth0"}"#).unwrap();
        store.append_event("failover", r#"{"bond": "test"}"#).unwrap();

        let events = store.get_recent_events(10).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "failover"); // Most recent first
    }

    #[test]
    fn test_health_history() {
        let store = StateStore::in_memory().unwrap();

        let entry = StoredHealthEntry {
            id: 0,
            interface: "eth0".into(),
            overall: 85.0,
            rtt: 95.0,
            jitter: 90.0,
            loss: 100.0,
            throughput: 60.0,
            stability: 80.0,
            timestamp: Utc::now(),
        };

        store.record_health(&entry).unwrap();

        let history = store.get_health_history("eth0", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert!((history[0].overall - 85.0).abs() < 0.01);
    }
}
