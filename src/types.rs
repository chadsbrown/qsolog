//! Shared primitive IDs and contest-related enums.

use serde::{Deserialize, Serialize};

/// Monotonic QSO identifier.
pub type QsoId = u64;
/// Monotonic operation sequence number.
pub type OpSeq = u64;
/// Contest instance identifier.
pub type ContestInstanceId = u64;
/// Radio identifier.
pub type RadioId = u32;
/// Operator identifier.
pub type OperatorId = u32;

/// HF contest band bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Band {
    /// 160 meters.
    B160m,
    /// 80 meters.
    B80m,
    /// 40 meters.
    B40m,
    /// 20 meters.
    B20m,
    /// 15 meters.
    B15m,
    /// 10 meters.
    B10m,
    /// Any non-standard band.
    Other,
}

/// Emission mode bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    /// Continuous Wave.
    CW,
    /// Single side-band phone.
    SSB,
    /// Any digital mode.
    Digital,
    /// Any non-standard mode.
    Other,
}
