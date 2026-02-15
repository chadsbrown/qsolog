use hashbrown::HashSet;

use crate::qso::QsoRecord;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DupeKey {
    pub call: String,
    pub band: crate::types::Band,
    pub mode: crate::types::Mode,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MultKey {
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SerialKey {
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DepKey {
    Dupe(DupeKey),
    Mult(MultKey),
    Serial(SerialKey),
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineApplied<Eval: Clone + PartialEq + Eq> {
    pub eval: Eval,
    pub deps: HashSet<DepKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalidation {
    pub keys_changed: Vec<DepKey>,
}

pub trait ContestEngine: Send + Sync + 'static {
    type State: Send;
    type Eval: Clone + PartialEq + Eq + Send;

    fn new_state(&self) -> Self::State;
    fn apply(&self, state: &mut Self::State, qso: &QsoRecord) -> EngineApplied<Self::Eval>;
    fn retract(&self, state: &mut Self::State, qso: &QsoRecord, applied: &EngineApplied<Self::Eval>);
    fn diff_invalidation(
        &self,
        old: &EngineApplied<Self::Eval>,
        new: &EngineApplied<Self::Eval>,
    ) -> Invalidation;
}
