//! SQLite-backed append-only op journal sink.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    core::store::{QsoStore, StoreSnapshotV1},
    op::{Op, StoredOp, StoredOpEnvelope},
    types::{OpSeq, QsoId},
};

use super::{OpSink, PersistError, PersistResult};

const SNAPSHOT_FORMAT_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotEnvelope {
    format_version: u16,
    snapshot: StoreSnapshotV1,
}

/// SQLite implementation of [`crate::persist::OpSink`].
pub struct SqliteOpSink {
    conn: Connection,
}

impl SqliteOpSink {
    /// Opens or creates a SQLite-backed sink at `path`.
    ///
    /// Enables WAL mode and sets `synchronous=NORMAL`.
    pub fn open(path: impl AsRef<Path>) -> PersistResult<Self> {
        let conn = Connection::open(path)?;
        Self::init_connection(conn)
    }

    /// Opens an in-memory SQLite sink.
    pub fn open_in_memory() -> PersistResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_connection(conn)
    }

    fn init_connection(conn: Connection) -> PersistResult<Self> {
        conn.execute_batch(include_str!("schema.sql"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self { conn })
    }

    /// Loads store state from latest snapshot plus tail events.
    pub fn load_store(&self) -> PersistResult<QsoStore> {
        let mut store = if let Some(snapshot) = self.load_latest_snapshot()? {
            QsoStore::from_snapshot(snapshot)?
        } else {
            QsoStore::new()
        };

        let start_seq = store.export_snapshot().next_op_seq.saturating_sub(1);
        let events = self.load_events_after(start_seq)?;
        for event in events {
            store.apply_replayed_op(event)?;
        }
        Ok(store)
    }

    /// Loads events strictly after `seq`.
    pub fn load_events_after(&self, seq: OpSeq) -> PersistResult<Vec<StoredOp>> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, ts_ms, payload FROM events WHERE seq > ?1 ORDER BY seq ASC")?;

        let rows = stmt.query_map(params![seq], |row| {
            let seq: i64 = row.get(0)?;
            let ts_ms: i64 = row.get(1)?;
            let payload: Vec<u8> = row.get(2)?;
            let mut op = decode_stored_op_payload(&payload).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    payload.len(),
                    rusqlite::types::Type::Blob,
                    Box::new(std::io::Error::other(err)),
                )
            })?;
            op.seq = seq as OpSeq;
            op.ts_ms = ts_ms as u64;
            Ok(op)
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Writes a snapshot covering `last_seq`.
    pub fn write_snapshot(
        &mut self,
        snapshot: &StoreSnapshotV1,
        last_seq: OpSeq,
    ) -> PersistResult<()> {
        let env = SnapshotEnvelope {
            format_version: SNAPSHOT_FORMAT_VERSION,
            snapshot: snapshot.clone(),
        };
        let payload = serde_json::to_vec(&env)?;
        let ts_ms = now_ms();
        self.conn.execute(
            "INSERT INTO snapshots(last_seq, ts_ms, payload) VALUES (?1, ?2, ?3)",
            params![last_seq as i64, ts_ms as i64, payload],
        )?;
        Ok(())
    }

    /// Deletes events up to and including `seq`.
    pub fn compact_through(&mut self, seq: OpSeq) -> PersistResult<usize> {
        let count = self
            .conn
            .execute("DELETE FROM events WHERE seq <= ?1", params![seq as i64])?;
        Ok(count)
    }

    /// Returns the latest sequence persisted in the events table.
    pub fn latest_seq(&self) -> PersistResult<OpSeq> {
        let seq: Option<i64> = self
            .conn
            .query_row("SELECT MAX(seq) FROM events", [], |row| row.get(0))
            .optional()?;
        Ok(seq.unwrap_or(0) as OpSeq)
    }

    fn load_latest_snapshot(&self) -> PersistResult<Option<StoreSnapshotV1>> {
        let payload: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT payload FROM snapshots ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        let Some(payload) = payload else {
            return Ok(None);
        };

        let env: SnapshotEnvelope = serde_json::from_slice(&payload)?;
        if env.format_version != SNAPSHOT_FORMAT_VERSION {
            return Err(PersistError::Message(
                "unsupported snapshot format".to_string(),
            ));
        }
        Ok(Some(env.snapshot))
    }
}

impl OpSink for SqliteOpSink {
    fn append_ops(&mut self, ops: &[StoredOp]) -> PersistResult<OpSeq> {
        if ops.is_empty() {
            return self.latest_seq();
        }

        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO events(seq, ts_ms, kind, qso_id, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for stored in ops {
                let payload = serde_json::to_vec(&StoredOpEnvelope::new(stored.clone()))?;
                let (kind, qso_id) = op_kind_and_id(&stored.op);
                stmt.execute(params![
                    stored.seq as i64,
                    stored.ts_ms as i64,
                    kind,
                    qso_id.map(|v| v as i64),
                    payload,
                ])?;
            }
        }
        tx.commit()?;

        Ok(ops.last().map(|o| o.seq).unwrap_or(0))
    }

    fn flush(&mut self) -> PersistResult<()> {
        self.conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        Ok(())
    }

    fn write_snapshot(&mut self, snapshot: &StoreSnapshotV1, last_seq: OpSeq) -> PersistResult<()> {
        SqliteOpSink::write_snapshot(self, snapshot, last_seq)
    }

    fn compact_through(&mut self, seq: OpSeq) -> PersistResult<usize> {
        SqliteOpSink::compact_through(self, seq)
    }
}

fn op_kind_and_id(op: &Op) -> (i64, Option<QsoId>) {
    match op {
        Op::Insert { qso } => (1, Some(qso.id)),
        Op::Patch { id, .. } => (2, Some(*id)),
        Op::Void { id, .. } => (3, Some(*id)),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn decode_stored_op_payload(payload: &[u8]) -> Result<StoredOp, String> {
    if let Ok(envelope) = serde_json::from_slice::<StoredOpEnvelope>(payload) {
        if envelope.format_version != crate::op::OP_FORMAT_VERSION {
            return Err(format!(
                "unsupported op format version: {}",
                envelope.format_version
            ));
        }
        return Ok(envelope.stored);
    }

    // Backward-compatible path for older payloads that stored raw StoredOp.
    serde_json::from_slice::<StoredOp>(payload)
        .map_err(|e| format!("op payload decode failed: {e}"))
}
