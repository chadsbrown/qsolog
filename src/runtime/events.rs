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
}
