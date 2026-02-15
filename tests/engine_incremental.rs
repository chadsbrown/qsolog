use hashbrown::{HashMap, HashSet};

use qsolog::{
    core::store::QsoStore,
    engine::{
        projector::Projector,
        traits::{
            ContestEngine, DepKey, DupeKey, EngineApplied, Invalidation, MultKey,
        },
    },
    qso::{ExchangeBlob, QsoDraft, QsoFlags, QsoPatch, QsoRecord},
    types::{Band, Mode, QsoId},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToyEval {
    points: u32,
    is_dupe: bool,
    is_new_mult: bool,
}

#[derive(Default)]
struct ToyState {
    dupe_counts: HashMap<DupeKey, usize>,
    mult_counts: HashMap<String, usize>,
}

#[derive(Default)]
struct ToyEngine;

impl ContestEngine for ToyEngine {
    type State = ToyState;
    type Eval = ToyEval;

    fn new_state(&self) -> Self::State {
        ToyState::default()
    }

    fn apply(&self, state: &mut Self::State, qso: &QsoRecord) -> EngineApplied<Self::Eval> {
        let dupe_key = DupeKey {
            call: qso.callsign_norm.clone(),
            band: qso.band,
            mode: qso.mode,
        };

        let mult_prefix = qso
            .callsign_norm
            .chars()
            .next()
            .unwrap_or('_')
            .to_string();
        let mult_bucket = format!("{:?}:{mult_prefix}", qso.band);

        let prev_dupe_count = *state.dupe_counts.get(&dupe_key).unwrap_or(&0);
        *state.dupe_counts.entry(dupe_key.clone()).or_insert(0) += 1;

        let prev_mult_count = *state.mult_counts.get(&mult_bucket).unwrap_or(&0);
        *state.mult_counts.entry(mult_bucket.clone()).or_insert(0) += 1;

        let mut deps = HashSet::new();
        deps.insert(DepKey::Dupe(dupe_key));
        deps.insert(DepKey::Mult(MultKey { key: mult_bucket }));

        EngineApplied {
            eval: ToyEval {
                points: if prev_dupe_count > 0 { 0 } else { 1 },
                is_dupe: prev_dupe_count > 0,
                is_new_mult: prev_mult_count == 0,
            },
            deps,
        }
    }

    fn retract(&self, state: &mut Self::State, _qso: &QsoRecord, applied: &EngineApplied<Self::Eval>) {
        for dep in &applied.deps {
            match dep {
                DepKey::Dupe(k) => {
                    if let Some(v) = state.dupe_counts.get_mut(k) {
                        *v = v.saturating_sub(1);
                        if *v == 0 {
                            state.dupe_counts.remove(k);
                        }
                    }
                }
                DepKey::Mult(k) => {
                    if let Some(v) = state.mult_counts.get_mut(&k.key) {
                        *v = v.saturating_sub(1);
                        if *v == 0 {
                            state.mult_counts.remove(&k.key);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn diff_invalidation(
        &self,
        old: &EngineApplied<Self::Eval>,
        new: &EngineApplied<Self::Eval>,
    ) -> Invalidation {
        let mut changed = Vec::new();

        for dep in old.deps.symmetric_difference(&new.deps) {
            changed.push(dep.clone());
        }

        if old.eval != new.eval {
            for dep in old.deps.union(&new.deps) {
                changed.push(dep.clone());
            }
        }

        Invalidation { keys_changed: changed }
    }
}

fn draft(call: &str) -> QsoDraft {
    QsoDraft {
        contest_instance_id: 7,
        callsign_raw: call.to_string(),
        callsign_norm: call.to_string(),
        band: Band::B20m,
        mode: Mode::CW,
        freq_hz: 14_010_000,
        ts_ms: 1,
        radio_id: 1,
        operator_id: 1,
        exchange: ExchangeBlob { bytes: vec![] },
        flags: QsoFlags::default(),
    }
}

fn full_recompute_projection(store: &QsoStore) -> HashMap<QsoId, EngineApplied<ToyEval>> {
    let engine = ToyEngine;
    let mut state = engine.new_state();
    let mut out = HashMap::new();

    for id in store.ordered_ids() {
        let Some(rec) = store.get(*id) else {
            continue;
        };
        if rec.flags.is_void {
            continue;
        }
        let applied = engine.apply(&mut state, rec);
        out.insert(*id, applied);
    }

    out
}

#[test]
fn incremental_projection_matches_full_recompute_and_undo_redo_restores() {
    let mut store = QsoStore::new();
    let mut projector = Projector::new(ToyEngine);

    let (a, op_a) = store.insert(draft("A1AA")).expect("insert a");
    projector.apply_stored_op(&store, &op_a).expect("proj a");

    let (_b, op_b) = store.insert(draft("B1BB")).expect("insert b");
    projector.apply_stored_op(&store, &op_b).expect("proj b");

    let (c, op_c) = store.insert(draft("C1CC")).expect("insert c");
    projector.apply_stored_op(&store, &op_c).expect("proj c");

    let before_patch = projector.applied().clone();

    let (_, patch_op) = store
        .patch(
            a,
            QsoPatch {
                callsign_raw: Some("C1CC".to_string()),
                callsign_norm: Some("C1CC".to_string()),
                ..QsoPatch::default()
            },
        )
        .expect("patch");
    projector
        .apply_stored_op(&store, &patch_op)
        .expect("proj patch");

    let after_patch = projector.applied().clone();
    let changed_ids: HashSet<QsoId> = before_patch
        .iter()
        .filter_map(|(id, before)| {
            let after = after_patch.get(id);
            if after != Some(before) {
                Some(*id)
            } else {
                None
            }
        })
        .collect();

    assert!(changed_ids.contains(&a));
    assert!(changed_ids.contains(&c));
    assert!(!changed_ids.contains(&2));

    let full = full_recompute_projection(&store);
    assert_eq!(full, after_patch);

    let (_, undo_op) = store.undo().expect("undo");
    projector.apply_stored_op(&store, &undo_op).expect("proj undo");
    assert_eq!(projector.applied().clone(), before_patch);

    let (_, redo_op) = store.redo().expect("redo");
    projector.apply_stored_op(&store, &redo_op).expect("proj redo");
    assert_eq!(projector.applied().clone(), after_patch);
}
