//! Mutation operation model and persistence wrappers.

use serde::{Deserialize, Serialize};

use crate::{
    qso::{QsoPatch, QsoRecord},
    types::{OpSeq, QsoId},
};

/// Version number for serialized [`StoredOpEnvelope`] payloads.
pub const OP_FORMAT_VERSION: u16 = 1;

/// Immutable operation appended to the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    /// Insert a fully materialized QSO.
    Insert {
        /// Inserted record.
        qso: QsoRecord,
    },
    /// Patch a record, including precomputed inverse patch.
    Patch {
        /// QSO id to mutate.
        id: QsoId,
        /// Forward patch.
        patch: QsoPatch,
        /// Inverse patch that restores prior state.
        prev: QsoPatch,
    },
    /// Toggle void state using previous value.
    Void {
        /// QSO id to mutate.
        id: QsoId,
        /// Previous void value.
        prev_is_void: bool,
    },
}

/// Journal row metadata plus operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOp {
    /// Monotonic operation sequence.
    pub seq: OpSeq,
    /// Operation timestamp in milliseconds.
    pub ts_ms: u64,
    /// Operation body.
    pub op: Op,
}

/// Versioned wrapper for stable on-disk payload decoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOpEnvelope {
    /// Payload format version.
    pub format_version: u16,
    /// Wrapped operation.
    pub stored: StoredOp,
}

impl StoredOpEnvelope {
    /// Constructs an envelope using [`OP_FORMAT_VERSION`].
    pub fn new(stored: StoredOp) -> Self {
        Self {
            format_version: OP_FORMAT_VERSION,
            stored,
        }
    }
}
