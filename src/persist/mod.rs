pub mod sqlite;

use crate::{
    core::store::StoreSnapshotV1,
    op::StoredOp,
    types::OpSeq,
};

#[derive(Debug)]
pub enum PersistError {
    Sqlite(rusqlite::Error),
    Serde(serde_json::Error),
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

pub type PersistResult<T> = Result<T, PersistError>;

pub trait OpSink: Send {
    fn append_ops(&mut self, ops: &[StoredOp]) -> PersistResult<OpSeq>;
    fn flush(&mut self) -> PersistResult<()> {
        Ok(())
    }
    fn write_snapshot(&mut self, _snapshot: &StoreSnapshotV1, _last_seq: OpSeq) -> PersistResult<()> {
        Ok(())
    }
    fn compact_through(&mut self, _seq: OpSeq) -> PersistResult<usize> {
        Ok(0)
    }
}
