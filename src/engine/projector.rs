use std::collections::VecDeque;

use hashbrown::{HashMap, HashSet};

use crate::{
    core::store::QsoStore,
    op::{Op, StoredOp},
    qso::QsoRecord,
    types::QsoId,
};

use super::traits::{ContestEngine, DepKey, EngineApplied};

#[derive(Debug)]
pub enum ProjectorError {
    MissingQso(QsoId),
}

pub struct Projector<E: ContestEngine> {
    engine: E,
    state: E::State,
    applied: HashMap<QsoId, EngineApplied<E::Eval>>,
    dep_index: HashMap<DepKey, HashSet<QsoId>>,
}

impl<E: ContestEngine> Projector<E> {
    pub fn new(engine: E) -> Self {
        let state = engine.new_state();
        Self {
            engine,
            state,
            applied: HashMap::new(),
            dep_index: HashMap::new(),
        }
    }

    pub fn applied(&self) -> &HashMap<QsoId, EngineApplied<E::Eval>> {
        &self.applied
    }

    pub fn apply_stored_op(&mut self, store: &QsoStore, stored: &StoredOp) -> Result<(), ProjectorError> {
        let (changed_id, old_record) = match &stored.op {
            Op::Insert { qso } => (qso.id, None),
            Op::Patch { id, prev, .. } => {
                let mut old = store.get_cloned(*id).ok_or(ProjectorError::MissingQso(*id))?;
                prev.apply_to(&mut old);
                (*id, Some(old))
            }
            Op::Void { id, prev_is_void } => {
                let mut old = store.get_cloned(*id).ok_or(ProjectorError::MissingQso(*id))?;
                old.flags.is_void = *prev_is_void;
                (*id, Some(old))
            }
        };

        self.incremental_reconcile(store, changed_id, old_record)
    }

    fn incremental_reconcile(
        &mut self,
        store: &QsoStore,
        changed_id: QsoId,
        old_record_for_changed: Option<QsoRecord>,
    ) -> Result<(), ProjectorError> {
        let mut impacted: HashSet<QsoId> = HashSet::new();
        impacted.insert(changed_id);

        let mut key_queue: VecDeque<DepKey> = VecDeque::new();
        if let Some(old_applied) = self.applied.get(&changed_id) {
            for dep in &old_applied.deps {
                key_queue.push_back(dep.clone());
            }
        }

        while let Some(key) = key_queue.pop_front() {
            if let Some(ids) = self.dep_index.get(&key) {
                for id in ids {
                    impacted.insert(*id);
                }
            }
        }

        let mut old_record_once = old_record_for_changed;

        loop {
            let changed_keys = self.recompute_impacted(store, &impacted, changed_id, old_record_once.as_ref())?;
            old_record_once = None;

            let mut expanded = false;
            for key in changed_keys {
                if let Some(ids) = self.dep_index.get(&key) {
                    for id in ids {
                        if impacted.insert(*id) {
                            expanded = true;
                        }
                    }
                }
            }

            if !expanded {
                break;
            }
        }

        Ok(())
    }

    fn recompute_impacted(
        &mut self,
        store: &QsoStore,
        impacted: &HashSet<QsoId>,
        changed_id: QsoId,
        old_record_for_changed: Option<&QsoRecord>,
    ) -> Result<HashSet<DepKey>, ProjectorError> {
        let mut old_applied_subset: HashMap<QsoId, EngineApplied<E::Eval>> = HashMap::new();

        for id in store.ordered_ids() {
            if !impacted.contains(id) {
                continue;
            }

            let Some(old_applied) = self.applied.remove(id) else {
                continue;
            };

            let rec_for_retract = if *id == changed_id {
                old_record_for_changed
                    .cloned()
                    .or_else(|| store.get_cloned(*id))
                    .ok_or(ProjectorError::MissingQso(*id))?
            } else {
                store.get_cloned(*id).ok_or(ProjectorError::MissingQso(*id))?
            };

            self.engine.retract(&mut self.state, &rec_for_retract, &old_applied);
            self.remove_dep_links(*id, &old_applied.deps);
            old_applied_subset.insert(*id, old_applied);
        }

        let mut new_applied_subset: HashMap<QsoId, EngineApplied<E::Eval>> = HashMap::new();

        for id in store.ordered_ids() {
            if !impacted.contains(id) {
                continue;
            }

            let rec = store.get(*id).ok_or(ProjectorError::MissingQso(*id))?;
            if rec.flags.is_void {
                continue;
            }

            let applied = self.engine.apply(&mut self.state, rec);
            self.add_dep_links(*id, &applied.deps);
            self.applied.insert(*id, applied.clone());
            new_applied_subset.insert(*id, applied);
        }

        let mut changed_keys: HashSet<DepKey> = HashSet::new();

        for id in impacted {
            let old_ap = old_applied_subset.get(id);
            let new_ap = new_applied_subset.get(id);

            match (old_ap, new_ap) {
                (Some(old), Some(new)) => {
                    if old != new {
                        let diff = self.engine.diff_invalidation(old, new);
                        for key in diff.keys_changed {
                            changed_keys.insert(key);
                        }
                    }
                }
                (Some(old), None) => {
                    for key in &old.deps {
                        changed_keys.insert(key.clone());
                    }
                }
                (None, Some(new)) => {
                    for key in &new.deps {
                        changed_keys.insert(key.clone());
                    }
                }
                (None, None) => {}
            }
        }

        Ok(changed_keys)
    }

    fn add_dep_links(&mut self, id: QsoId, deps: &HashSet<DepKey>) {
        for dep in deps {
            self.dep_index.entry(dep.clone()).or_default().insert(id);
        }
    }

    fn remove_dep_links(&mut self, id: QsoId, deps: &HashSet<DepKey>) {
        for dep in deps {
            if let Some(ids) = self.dep_index.get_mut(dep) {
                ids.remove(&id);
                if ids.is_empty() {
                    self.dep_index.remove(dep);
                }
            }
        }
    }
}
