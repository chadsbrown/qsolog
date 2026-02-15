//! Persistence abstractions and sink implementations.

/// SQLite sink implementation.
pub mod sqlite;

use crate::{core::store::StoreSnapshotV1, op::StoredOp, types::OpSeq};

/// Persistence-layer error type.
#[derive(Debug)]
pub enum PersistError {
    /// Wrapped SQLite error.
    Sqlite(rusqlite::Error),
    /// Wrapped serialization error.
    Serde(serde_json::Error),
    /// Generic message error.
    Message(String),
}

impl From<rusqlite::Error> for PersistError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<serde_json::Error> for PersistError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

impl From<crate::core::store::StoreError> for PersistError {
    fn from(value: crate::core::store::StoreError) -> Self {
        Self::Message(format!("store error: {value:?}"))
    }
}

/// Result alias for persistence operations.
pub type PersistResult<T> = Result<T, PersistError>;

/// Append-only operation sink abstraction.
pub trait OpSink: Send {
    /// Appends one batch of operations and returns durable sequence.
    fn append_ops(&mut self, ops: &[StoredOp]) -> PersistResult<OpSeq>;
    /// Flushes any buffered durability work.
    fn flush(&mut self) -> PersistResult<()> {
        Ok(())
    }
    /// Writes a snapshot with the given last covered sequence.
    fn write_snapshot(
        &mut self,
        _snapshot: &StoreSnapshotV1,
        _last_seq: OpSeq,
    ) -> PersistResult<()> {
        Ok(())
    }
    /// Compacts journal data through `seq`.
    fn compact_through(&mut self, _seq: OpSeq) -> PersistResult<usize> {
        Ok(0)
    }
}
