//! QSO domain record, draft, flags, and patch types.

use serde::{Deserialize, Serialize};

use crate::types::{Band, ContestInstanceId, OperatorId, QsoId, RadioId};

/// Opaque serialized exchange payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExchangeBlob {
    /// Raw exchange bytes.
    pub bytes: Vec<u8>,
}

/// Record flags that affect scoring and visibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QsoFlags {
    /// True when the QSO is voided.
    pub is_void: bool,
    /// True when a dupe should still score.
    pub dupe_override: bool,
}

/// Fully materialized, authoritative QSO record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QsoRecord {
    /// Stable QSO identifier.
    pub id: QsoId,
    /// Contest instance to which this QSO belongs.
    pub contest_instance_id: ContestInstanceId,
    /// Operator-entered callsign text.
    pub callsign_raw: String,
    /// Normalized callsign used for indexing/scoring.
    pub callsign_norm: String,
    /// Band bucket.
    pub band: Band,
    /// Mode bucket.
    pub mode: crate::types::Mode,
    /// Frequency in Hz.
    pub freq_hz: u64,
    /// Timestamp in milliseconds since epoch.
    pub ts_ms: u64,
    /// Radio identifier.
    pub radio_id: RadioId,
    /// Operator identifier.
    pub operator_id: OperatorId,
    /// Opaque exchange content.
    pub exchange: ExchangeBlob,
    /// Record flags.
    pub flags: QsoFlags,
}

/// Insert payload used to create a new [`QsoRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QsoDraft {
    /// Contest instance to which this QSO belongs.
    pub contest_instance_id: ContestInstanceId,
    /// Operator-entered callsign text.
    pub callsign_raw: String,
    /// Normalized callsign used for indexing/scoring.
    pub callsign_norm: String,
    /// Band bucket.
    pub band: Band,
    /// Mode bucket.
    pub mode: crate::types::Mode,
    /// Frequency in Hz.
    pub freq_hz: u64,
    /// Timestamp in milliseconds since epoch.
    pub ts_ms: u64,
    /// Radio identifier.
    pub radio_id: RadioId,
    /// Operator identifier.
    pub operator_id: OperatorId,
    /// Opaque exchange content.
    pub exchange: ExchangeBlob,
    /// Record flags.
    pub flags: QsoFlags,
}

/// Sparse patch where each `Some` field overwrites the record value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct QsoPatch {
    /// Optional replacement for contest instance id.
    pub contest_instance_id: Option<ContestInstanceId>,
    /// Optional replacement for raw callsign.
    pub callsign_raw: Option<String>,
    /// Optional replacement for normalized callsign.
    pub callsign_norm: Option<String>,
    /// Optional replacement for band.
    pub band: Option<Band>,
    /// Optional replacement for mode.
    pub mode: Option<crate::types::Mode>,
    /// Optional replacement for frequency.
    pub freq_hz: Option<u64>,
    /// Optional replacement for timestamp.
    pub ts_ms: Option<u64>,
    /// Optional replacement for radio id.
    pub radio_id: Option<RadioId>,
    /// Optional replacement for operator id.
    pub operator_id: Option<OperatorId>,
    /// Optional replacement for exchange.
    pub exchange: Option<ExchangeBlob>,
    /// Optional replacement for void flag.
    pub is_void: Option<bool>,
    /// Optional replacement for dupe override flag.
    pub dupe_override: Option<bool>,
}

impl QsoPatch {
    /// Returns true when no fields are set.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Captures an inverse patch for all fields present in `self`.
    pub fn capture_inverse_for(&self, rec: &QsoRecord) -> Self {
        Self {
            contest_instance_id: self.contest_instance_id.map(|_| rec.contest_instance_id),
            callsign_raw: self.callsign_raw.as_ref().map(|_| rec.callsign_raw.clone()),
            callsign_norm: self
                .callsign_norm
                .as_ref()
                .map(|_| rec.callsign_norm.clone()),
            band: self.band.map(|_| rec.band),
            mode: self.mode.map(|_| rec.mode),
            freq_hz: self.freq_hz.map(|_| rec.freq_hz),
            ts_ms: self.ts_ms.map(|_| rec.ts_ms),
            radio_id: self.radio_id.map(|_| rec.radio_id),
            operator_id: self.operator_id.map(|_| rec.operator_id),
            exchange: self.exchange.as_ref().map(|_| rec.exchange.clone()),
            is_void: self.is_void.map(|_| rec.flags.is_void),
            dupe_override: self.dupe_override.map(|_| rec.flags.dupe_override),
        }
    }

    /// Applies this patch in place to `rec`.
    pub fn apply_to(&self, rec: &mut QsoRecord) {
        if let Some(v) = self.contest_instance_id {
            rec.contest_instance_id = v;
        }
        if let Some(v) = &self.callsign_raw {
            rec.callsign_raw = v.clone();
        }
        if let Some(v) = &self.callsign_norm {
            rec.callsign_norm = v.clone();
        }
        if let Some(v) = self.band {
            rec.band = v;
        }
        if let Some(v) = self.mode {
            rec.mode = v;
        }
        if let Some(v) = self.freq_hz {
            rec.freq_hz = v;
        }
        if let Some(v) = self.ts_ms {
            rec.ts_ms = v;
        }
        if let Some(v) = self.radio_id {
            rec.radio_id = v;
        }
        if let Some(v) = self.operator_id {
            rec.operator_id = v;
        }
        if let Some(v) = &self.exchange {
            rec.exchange = v.clone();
        }
        if let Some(v) = self.is_void {
            rec.flags.is_void = v;
        }
        if let Some(v) = self.dupe_override {
            rec.flags.dupe_override = v;
        }
    }
}
