use hashbrown::HashMap;

use crate::types::QsoId;

pub type VecIndex<K> = HashMap<K, Vec<QsoId>>;
