use serde::{Deserialize, Serialize};

pub type QsoId = u64;
pub type OpSeq = u64;
pub type ContestInstanceId = u64;
pub type RadioId = u32;
pub type OperatorId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Band {
    B160m,
    B80m,
    B40m,
    B20m,
    B15m,
    B10m,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    CW,
    SSB,
    Digital,
    Other,
}
