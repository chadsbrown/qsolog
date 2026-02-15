use serde::{Deserialize, Serialize};

use crate::types::{Band, ContestInstanceId, OperatorId, QsoId, RadioId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExchangeBlob {
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QsoFlags {
    pub is_void: bool,
    pub dupe_override: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QsoRecord {
    pub id: QsoId,
    pub contest_instance_id: ContestInstanceId,
    pub callsign_raw: String,
    pub callsign_norm: String,
    pub band: Band,
    pub mode: crate::types::Mode,
    pub freq_hz: u64,
    pub ts_ms: u64,
    pub radio_id: RadioId,
    pub operator_id: OperatorId,
    pub exchange: ExchangeBlob,
    pub flags: QsoFlags,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QsoDraft {
    pub contest_instance_id: ContestInstanceId,
    pub callsign_raw: String,
    pub callsign_norm: String,
    pub band: Band,
    pub mode: crate::types::Mode,
    pub freq_hz: u64,
    pub ts_ms: u64,
    pub radio_id: RadioId,
    pub operator_id: OperatorId,
    pub exchange: ExchangeBlob,
    pub flags: QsoFlags,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct QsoPatch {
    pub contest_instance_id: Option<ContestInstanceId>,
    pub callsign_raw: Option<String>,
    pub callsign_norm: Option<String>,
    pub band: Option<Band>,
    pub mode: Option<crate::types::Mode>,
    pub freq_hz: Option<u64>,
    pub ts_ms: Option<u64>,
    pub radio_id: Option<RadioId>,
    pub operator_id: Option<OperatorId>,
    pub exchange: Option<ExchangeBlob>,
    pub is_void: Option<bool>,
    pub dupe_override: Option<bool>,
}

impl QsoPatch {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

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
