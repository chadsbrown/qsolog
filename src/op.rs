use serde::{Deserialize, Serialize};

use crate::{
    qso::{QsoPatch, QsoRecord},
    types::{OpSeq, QsoId},
};

pub const OP_FORMAT_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Insert {
        qso: QsoRecord,
    },
    Patch {
        id: QsoId,
        patch: QsoPatch,
        prev: QsoPatch,
    },
    Void {
        id: QsoId,
        prev_is_void: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOp {
    pub seq: OpSeq,
    pub ts_ms: u64,
    pub op: Op,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOpEnvelope {
    pub format_version: u16,
    pub stored: StoredOp,
}

impl StoredOpEnvelope {
    pub fn new(stored: StoredOp) -> Self {
        Self {
            format_version: OP_FORMAT_VERSION,
            stored,
        }
    }
}
