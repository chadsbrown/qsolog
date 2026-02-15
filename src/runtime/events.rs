use crate::types::{OpSeq, QsoId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QsoEvent {
    Inserted { id: QsoId },
    Updated { id: QsoId },
    Voided { id: QsoId },
    UndoApplied,
    RedoApplied,
    DurableUpTo { op_seq: OpSeq },
}
