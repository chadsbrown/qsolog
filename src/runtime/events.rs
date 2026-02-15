//! Runtime event stream payloads.

use crate::types::{OpSeq, QsoId};

/// Events emitted from the single-writer runtime loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QsoEvent {
    /// A new QSO was inserted.
    Inserted {
        /// Inserted QSO id.
        id: QsoId,
    },
    /// An existing QSO was updated.
    Updated {
        /// Updated QSO id.
        id: QsoId,
    },
    /// A QSO was voided.
    Voided {
        /// Voided QSO id.
        id: QsoId,
    },
    /// One undo step was applied.
    UndoApplied,
    /// One redo step was applied.
    RedoApplied,
    /// Persistence has reached at least this op sequence.
    DurableUpTo {
        /// Highest sequence known durable.
        op_seq: OpSeq,
    },
    /// Persistence worker reported an error and runtime marked persistence unhealthy.
    PersistenceError {
        /// Human-readable persistence error.
        error: String,
        /// Last sequence known durable when the error occurred.
        last_durable_seq: OpSeq,
    },
    /// Mutation was accepted in memory while durability was unhealthy.
    NotDurableWarning {
        /// Sequence for the mutation that may be non-durable.
        op_seq: OpSeq,
    },
}
