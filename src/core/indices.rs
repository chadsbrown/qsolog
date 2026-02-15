//! Common index aliases used by the in-memory store.

use hashbrown::HashMap;

use crate::types::QsoId;

/// A vector-backed secondary index keyed by `K`.
pub type VecIndex<K> = HashMap<K, Vec<QsoId>>;
