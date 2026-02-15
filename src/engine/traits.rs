//! Contest-engine traits for incremental QSO evaluation.

use hashbrown::HashSet;

use crate::qso::QsoRecord;

/// Duplicate detection key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DupeKey {
    /// Normalized callsign.
    pub call: String,
    /// Band component.
    pub band: crate::types::Band,
    /// Mode component.
    pub mode: crate::types::Mode,
}

/// Multiplier key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MultKey {
    /// Engine-defined key string.
    pub key: String,
}

/// Serial-number dependency key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SerialKey {
    /// Engine-defined key string.
    pub key: String,
}

/// Dependency keys whose changes may invalidate other QSO evaluations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DepKey {
    /// Dupe dependency.
    Dupe(DupeKey),
    /// Mult dependency.
    Mult(MultKey),
    /// Serial dependency.
    Serial(SerialKey),
    /// Arbitrary engine dependency.
    Custom(String),
}

/// Cached engine result for one applied QSO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineApplied<Eval: Clone + PartialEq + Eq> {
    /// Engine evaluation output.
    pub eval: Eval,
    /// Dependency keys touched by this evaluation.
    pub deps: HashSet<DepKey>,
}

/// Invalidation output from diffing old vs new applied state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalidation {
    /// Keys whose dependent QSOs should be reconsidered.
    pub keys_changed: Vec<DepKey>,
}

/// Contest scoring engine abstraction.
pub trait ContestEngine: Send + Sync + 'static {
    /// Mutable engine state type.
    type State: Send;
    /// Per-QSO evaluation output type.
    type Eval: Clone + PartialEq + Eq + Send;

    /// Creates a fresh engine state.
    fn new_state(&self) -> Self::State;
    /// Applies one QSO into state and returns its cached output.
    fn apply(&self, state: &mut Self::State, qso: &QsoRecord) -> EngineApplied<Self::Eval>;
    /// Retracts one previously applied QSO from state.
    fn retract(
        &self,
        state: &mut Self::State,
        qso: &QsoRecord,
        applied: &EngineApplied<Self::Eval>,
    );
    /// Computes invalidation keys between two cached results.
    fn diff_invalidation(
        &self,
        old: &EngineApplied<Self::Eval>,
        new: &EngineApplied<Self::Eval>,
    ) -> Invalidation;
}
