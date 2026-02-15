use tempfile::TempDir;

use qsolog::{
    core::store::QsoStore,
    persist::{sqlite::SqliteOpSink, OpSink},
    qso::{ExchangeBlob, QsoDraft, QsoFlags, QsoPatch},
    types::{Band, Mode},
};

fn draft(call: &str, ts: u64) -> QsoDraft {
    QsoDraft {
        contest_instance_id: 42,
        callsign_raw: call.to_string(),
        callsign_norm: call.to_string(),
        band: Band::B40m,
        mode: Mode::CW,
        freq_hz: 7_020_000,
        ts_ms: ts,
        radio_id: 1,
        operator_id: 1,
        exchange: ExchangeBlob { bytes: vec![] },
        flags: QsoFlags::default(),
    }
}

#[test]
fn sqlite_replay_round_trips_state_and_order() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("ops.db");

    let mut store = QsoStore::new();
    let mut sink = SqliteOpSink::open(&db_path).expect("open sqlite");

    let (id1, _) = store.insert(draft("K1AAA", 1)).expect("insert1");
    let (id2, _) = store.insert(draft("K2BBB", 2)).expect("insert2");
    let (_, _) = store
        .patch(
            id1,
            QsoPatch {
                callsign_raw: Some("K1ZZZ".to_string()),
                callsign_norm: Some("K1ZZZ".to_string()),
                ..QsoPatch::default()
            },
        )
        .expect("patch");
    let (_, _) = store.void(id2).expect("void");

    let ops = store.drain_pending_ops();
    sink.append_ops(&ops).expect("append");

    drop(sink);

    let sink2 = SqliteOpSink::open(&db_path).expect("reopen");
    let replayed = sink2.load_store().expect("replay");

    let orig = store.export_snapshot();
    let replay = replayed.export_snapshot();
    assert_eq!(orig.order, replay.order);
    assert_eq!(orig.records, replay.records);
}

#[test]
fn snapshot_and_compaction_preserve_replay() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("snap.db");

    let mut store = QsoStore::new();
    let mut sink = SqliteOpSink::open(&db_path).expect("open sqlite");

    for i in 0..10u64 {
        let _ = store.insert(draft(&format!("N{i}CALL"), i)).expect("insert");
    }
    sink.append_ops(&store.drain_pending_ops()).expect("append");

    let snapshot = store.export_snapshot();
    let last_seq = store.latest_op_seq();
    sink.write_snapshot(&snapshot, last_seq).expect("snapshot");
    let removed = sink.compact_through(last_seq).expect("compact");
    assert!(removed > 0);

    drop(sink);

    let reopened = SqliteOpSink::open(&db_path).expect("reopen");
    let replayed = reopened.load_store().expect("replay");

    assert_eq!(replayed.export_snapshot().order, snapshot.order);
    assert_eq!(replayed.export_snapshot().records, snapshot.records);
}
