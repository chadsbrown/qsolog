use qsolog::{
    core::store::QsoStore,
    qso::{ExchangeBlob, QsoDraft, QsoFlags, QsoPatch},
    types::{Band, Mode},
};

fn draft(call_raw: &str, call_norm: &str, ts_ms: u64) -> QsoDraft {
    QsoDraft {
        contest_instance_id: 1,
        callsign_raw: call_raw.to_string(),
        callsign_norm: call_norm.to_string(),
        band: Band::B20m,
        mode: Mode::CW,
        freq_hz: 14_025_000,
        ts_ms,
        radio_id: 1,
        operator_id: 1,
        exchange: ExchangeBlob {
            bytes: b"599 MA".to_vec(),
        },
        flags: QsoFlags::default(),
    }
}

#[test]
fn insert_yields_monotonic_ids() {
    let mut store = QsoStore::new();
    let (id1, op1) = store.insert(draft("K1ABC", "K1ABC", 1)).unwrap();
    let (id2, op2) = store.insert(draft("K2DEF", "K2DEF", 2)).unwrap();
    let (id3, op3) = store.insert(draft("K3GHI", "K3GHI", 3)).unwrap();

    assert_eq!((id1, id2, id3), (1, 2, 3));
    assert_eq!((op1.seq, op2.seq, op3.seq), (1, 2, 3));
}

#[test]
fn patch_undo_redo_restores_exact_state() {
    let mut store = QsoStore::new();
    let (id, _) = store.insert(draft("K1ABC", "K1ABC", 10)).unwrap();

    let before = store.get(id).unwrap().clone();

    let patch = QsoPatch {
        callsign_raw: Some("K1XYZ".to_string()),
        callsign_norm: Some("K1XYZ".to_string()),
        freq_hz: Some(14_030_000),
        dupe_override: Some(true),
        ..QsoPatch::default()
    };

    store.patch(id, patch).unwrap();
    let after_patch = store.get(id).unwrap().clone();
    assert_ne!(after_patch, before);

    store.undo().unwrap();
    let after_undo = store.get(id).unwrap().clone();
    assert_eq!(after_undo, before);

    store.redo().unwrap();
    let after_redo = store.get(id).unwrap().clone();
    assert_eq!(after_redo, after_patch);
}
