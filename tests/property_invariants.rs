use std::collections::BTreeSet;

use proptest::prelude::*;

use qsolog::{
    core::store::{QsoStore, StoreError},
    qso::{ExchangeBlob, QsoDraft, QsoFlags, QsoPatch, QsoRecord},
    types::{Band, Mode, QsoId},
};

#[derive(Debug, Clone)]
enum Action {
    Insert { call_idx: u8, ts: u16 },
    PatchCall { target: u8, call_idx: u8 },
    PatchFreq { target: u8, freq: u32 },
    Void { target: u8 },
}

fn action_strategy() -> impl Strategy<Value = Action> {
    prop_oneof![
        (0u8..24, 0u16..5000).prop_map(|(call_idx, ts)| Action::Insert { call_idx, ts }),
        (0u8..24, 0u8..24).prop_map(|(target, call_idx)| Action::PatchCall { target, call_idx }),
        (0u8..24, 7_000_000u32..30_000_000u32)
            .prop_map(|(target, freq)| Action::PatchFreq { target, freq }),
        (0u8..24).prop_map(|target| Action::Void { target }),
    ]
}

fn draft_from(call_idx: u8, ts: u16) -> QsoDraft {
    let call = format!("K{call_idx}AA");
    QsoDraft {
        contest_instance_id: 1,
        callsign_raw: call.clone(),
        callsign_norm: call,
        band: Band::B20m,
        mode: Mode::CW,
        freq_hz: 14_000_000 + u64::from(call_idx),
        ts_ms: u64::from(ts),
        radio_id: 1,
        operator_id: 1,
        exchange: ExchangeBlob { bytes: vec![] },
        flags: QsoFlags::default(),
    }
}

fn all_ids(store: &QsoStore) -> Vec<QsoId> {
    store.ordered_ids().to_vec()
}

fn full_scan_by_call(store: &QsoStore, call: &str) -> Vec<QsoId> {
    store
        .ordered_ids()
        .iter()
        .copied()
        .filter(|id| store.get(*id).is_some_and(|r| r.callsign_norm == call))
        .collect()
}

fn by_call_ids(store: &QsoStore, call: &str) -> Vec<QsoId> {
    store.by_call(call).into_iter().map(|r| r.id).collect()
}

fn snapshot_records(store: &QsoStore) -> Vec<QsoRecord> {
    store
        .ordered_ids()
        .iter()
        .filter_map(|id| store.get(*id).cloned())
        .collect()
}

proptest! {
    #[test]
    fn random_sequences_preserve_indices_and_undo_redo_roundtrip(actions in prop::collection::vec(action_strategy(), 1..200)) {
        let mut store = QsoStore::new();
        let mut calls = BTreeSet::<String>::new();

        for action in actions {
            match action {
                Action::Insert { call_idx, ts } => {
                    let call = format!("K{call_idx}AA");
                    calls.insert(call.clone());
                    let _ = store.insert(draft_from(call_idx, ts));
                }
                Action::PatchCall { target, call_idx } => {
                    let ids = all_ids(&store);
                    if ids.is_empty() {
                        continue;
                    }
                    let id = ids[usize::from(target) % ids.len()];
                    let call = format!("K{call_idx}AA");
                    calls.insert(call.clone());
                    let _ = store.patch(
                        id,
                        QsoPatch {
                            callsign_raw: Some(call.clone()),
                            callsign_norm: Some(call),
                            ..QsoPatch::default()
                        },
                    );
                }
                Action::PatchFreq { target, freq } => {
                    let ids = all_ids(&store);
                    if ids.is_empty() {
                        continue;
                    }
                    let id = ids[usize::from(target) % ids.len()];
                    let _ = store.patch(
                        id,
                        QsoPatch {
                            freq_hz: Some(u64::from(freq)),
                            ..QsoPatch::default()
                        },
                    );
                }
                Action::Void { target } => {
                    let ids = all_ids(&store);
                    if ids.is_empty() {
                        continue;
                    }
                    let id = ids[usize::from(target) % ids.len()];
                    let _ = store.void(id);
                }
            }

            for call in &calls {
                prop_assert_eq!(by_call_ids(&store, call), full_scan_by_call(&store, call));
            }
        }

        let target = snapshot_records(&store);
        loop {
            match store.undo() {
                Ok(_) => {},
                Err(StoreError::NothingToUndo) => break,
                Err(other) => prop_assert!(false, "unexpected undo error: {other:?}"),
            }
        }

        loop {
            match store.redo() {
                Ok(_) => {},
                Err(StoreError::NothingToRedo) => break,
                Err(other) => prop_assert!(false, "unexpected redo error: {other:?}"),
            }
        }

        prop_assert_eq!(snapshot_records(&store), target);
    }
}
