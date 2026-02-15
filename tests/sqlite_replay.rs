use rusqlite::Connection;
use tempfile::TempDir;

use qsolog::{
    core::store::QsoStore,
    persist::{OpSink, sqlite::SqliteOpSink},
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
        let _ = store
            .insert(draft(&format!("N{i}CALL"), i))
            .expect("insert");
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

#[test]
fn replay_from_snapshot_plus_tail_events_matches_exact_state() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("tail.db");

    let mut store = QsoStore::new();
    let mut sink = SqliteOpSink::open(&db_path).expect("open sqlite");

    for i in 0..6u64 {
        let _ = store.insert(draft(&format!("W{i}AAA"), i)).expect("insert");
    }
    sink.append_ops(&store.drain_pending_ops())
        .expect("append seed");

    let snapshot = store.export_snapshot();
    let snapshot_seq = store.latest_op_seq();
    sink.write_snapshot(&snapshot, snapshot_seq)
        .expect("snapshot");

    let (_, _) = store
        .patch(
            2,
            QsoPatch {
                callsign_raw: Some("W2ZZZ".to_string()),
                callsign_norm: Some("W2ZZZ".to_string()),
                ..QsoPatch::default()
            },
        )
        .expect("patch tail");
    let (_, _) = store.void(4).expect("void tail");
    let _ = store.insert(draft("W9NEW", 99)).expect("insert tail");

    sink.append_ops(&store.drain_pending_ops())
        .expect("append tail");
    drop(sink);

    let reopened = SqliteOpSink::open(&db_path).expect("reopen");
    let replayed = reopened.load_store().expect("replay");

    assert_eq!(
        replayed.export_snapshot().order,
        store.export_snapshot().order
    );
    assert_eq!(
        replayed.export_snapshot().records,
        store.export_snapshot().records
    );
}

#[test]
fn open_migrates_or_initializes_meta_versions() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("meta_init.db");

    let conn = Connection::open(&db_path).expect("open");
    conn.execute_batch(include_str!("../src/persist/schema.sql"))
        .expect("schema");
    conn.execute(
        "INSERT INTO meta(key, value) VALUES ('schema_version', '0')",
        [],
    )
    .expect("insert meta");
    drop(conn);

    let _sink = SqliteOpSink::open(&db_path).expect("open with migration");

    let check = Connection::open(&db_path).expect("reopen");
    let schema: String = check
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .expect("schema version");
    let op_fmt: String = check
        .query_row(
            "SELECT value FROM meta WHERE key='op_format_version'",
            [],
            |r| r.get(0),
        )
        .expect("op format");
    let snap_fmt: String = check
        .query_row(
            "SELECT value FROM meta WHERE key='snapshot_format_version'",
            [],
            |r| r.get(0),
        )
        .expect("snapshot format");
    let station_id: String = check
        .query_row(
            "SELECT value FROM meta WHERE key='station_instance_id'",
            [],
            |r| r.get(0),
        )
        .expect("station id");

    assert_eq!(schema, "1");
    assert!(!op_fmt.is_empty());
    assert!(!snap_fmt.is_empty());
    assert_eq!(station_id, "local");
}

#[test]
fn open_fails_on_unsupported_schema_version() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("meta_bad.db");

    let conn = Connection::open(&db_path).expect("open");
    conn.execute_batch(include_str!("../src/persist/schema.sql"))
        .expect("schema");
    conn.execute(
        "INSERT INTO meta(key, value) VALUES ('schema_version', '999')",
        [],
    )
    .expect("insert meta");
    drop(conn);

    let err = match SqliteOpSink::open(&db_path) {
        Ok(_) => panic!("should fail"),
        Err(err) => err,
    };
    match err {
        qsolog::persist::PersistError::Message(msg) => {
            assert!(msg.contains("unsupported schema version"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
