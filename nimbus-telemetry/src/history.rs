use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};

use nimbus_core::error::Result;
use nimbus_core::types::ConnectionRecord;

pub struct HistoryDb {
    conn: Connection,
}

impl HistoryDb {
    pub fn open() -> Result<Self> {
        let path = db_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS connections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hotspot_uuid TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                interface TEXT NOT NULL,
                stations_connected INTEGER DEFAULT 0,
                total_rx_bytes INTEGER DEFAULT 0,
                total_tx_bytes INTEGER DEFAULT 0
            );",
        )?;
        Ok(Self { conn })
    }

    pub fn start_record(&self, hotspot_uuid: &str, interface: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO connections (hotspot_uuid, started_at, interface) VALUES (?1, ?2, ?3)",
            params![hotspot_uuid, Utc::now().to_rfc3339(), interface],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn end_record(
        &self,
        id: i64,
        stations: u32,
        rx_bytes: u64,
        tx_bytes: u64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE connections SET ended_at = ?1, stations_connected = ?2, total_rx_bytes = ?3, total_tx_bytes = ?4 WHERE id = ?5",
            params![Utc::now().to_rfc3339(), stations, rx_bytes, tx_bytes, id],
        )?;
        Ok(())
    }

    pub fn get_recent(&self, limit: usize) -> Result<Vec<ConnectionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, hotspot_uuid, started_at, ended_at, interface, stations_connected, total_rx_bytes, total_tx_bytes
             FROM connections ORDER BY started_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(ConnectionRecord {
                id: row.get(0)?,
                hotspot_uuid: row.get(1)?,
                started_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                ended_at: row.get::<_, Option<String>>(3)?.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
                interface: row.get(4)?,
                stations_connected: row.get(5)?,
                total_rx_bytes: row.get(6)?,
                total_tx_bytes: row.get(7)?,
            })
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }
}

fn db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/nimbus-hotspot/history.db")
}
