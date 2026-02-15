use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use qsolog::{
    core::store::QsoStore,
    persist::OpSink,
    qso::{ExchangeBlob, QsoDraft, QsoFlags, QsoPatch},
    runtime::{
        events::QsoEvent,
        handle::{AckMode, RuntimeConfig, RuntimeError, spawn_qsolog},
    },
    types::{Band, Mode, OpSeq},
};

fn draft(call: &str, ts_ms: u64) -> QsoDraft {
    QsoDraft {
        contest_instance_id: 1,
        callsign_raw: call.to_string(),
        callsign_norm: call.to_string(),
        band: Band::B20m,
        mode: Mode::CW,
        freq_hz: 14_000_000,
        ts_ms,
        radio_id: 1,
        operator_id: 1,
        exchange: ExchangeBlob { bytes: vec![] },
        flags: QsoFlags::default(),
    }
}

struct SlowSink {
    seen: Arc<Mutex<Vec<OpSeq>>>,
    delay: Duration,
}

impl OpSink for SlowSink {
    fn append_ops(
        &mut self,
        ops: &[qsolog::op::StoredOp],
    ) -> qsolog::persist::PersistResult<OpSeq> {
        std::thread::sleep(self.delay);
        let mut seen = self.seen.lock().expect("lock");
        for op in ops {
            seen.push(op.seq);
        }
        Ok(ops.last().map(|o| o.seq).unwrap_or(0))
    }
}

struct FailingSink {
    append_calls: usize,
    fail_after: usize,
}

impl OpSink for FailingSink {
    fn append_ops(
        &mut self,
        ops: &[qsolog::op::StoredOp],
    ) -> qsolog::persist::PersistResult<OpSeq> {
        self.append_calls += 1;
        if self.append_calls > self.fail_after {
            return Err(qsolog::persist::PersistError::Message(
                "forced append failure".to_string(),
            ));
        }
        Ok(ops.last().map(|o| o.seq).unwrap_or(0))
    }
}

#[tokio::test]
async fn runtime_insert_patch_query_and_events_ordered() {
    let handle = spawn_qsolog(QsoStore::new(), None, RuntimeConfig::default());
    let mut sub = handle.subscribe();

    let id = handle.insert(draft("K1ABC", 1)).await.expect("insert");
    handle
        .patch(
            id,
            QsoPatch {
                callsign_raw: Some("K1XYZ".to_string()),
                callsign_norm: Some("K1XYZ".to_string()),
                ..QsoPatch::default()
            },
        )
        .await
        .expect("patch");

    let rec = handle.get(id).await.expect("get").expect("record");
    assert_eq!(rec.callsign_norm, "K1XYZ");

    let mut seen = Vec::new();
    for _ in 0..6 {
        let evt = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("event")
            .expect("recv");
        if !matches!(evt, QsoEvent::DurableUpTo { .. }) {
            seen.push(evt);
        }
        if seen.len() == 2 {
            break;
        }
    }

    assert_eq!(seen[0], QsoEvent::Inserted { id });
    assert_eq!(seen[1], QsoEvent::Updated { id });

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn durable_event_advances_and_slow_sink_surfaces_queue_pressure() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = SlowSink {
        seen: Arc::clone(&seen),
        delay: Duration::from_millis(250),
    };

    let cfg = RuntimeConfig {
        ack_mode: AckMode::InMemory,
        flush_on_insert: true,
        batch_max_ops: 16,
        batch_max_latency_ms: 500,
        persist_queue_bound: 1,
        snapshot_every_ops: 0,
        compact_after_snapshot: false,
    };

    let handle = spawn_qsolog(QsoStore::new(), Some(Box::new(sink)), cfg);
    let mut sub = handle.subscribe();

    let id = handle.insert(draft("N0CALL", 1)).await.expect("insert");
    assert_eq!(id, 1);

    let mut durable_seen = false;
    for _ in 0..5 {
        let evt = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("recv timeout")
            .expect("recv");
        if matches!(evt, QsoEvent::DurableUpTo { .. }) {
            durable_seen = true;
            break;
        }
    }
    assert!(durable_seen, "expected DurableUpTo event");

    let mut queue_error_seen = false;
    for i in 0..12u64 {
        let r = handle.insert(draft(&format!("K{i}"), i + 2)).await;
        if let Err(RuntimeError::PersistQueueFull) = r {
            queue_error_seen = true;
            break;
        }
    }
    assert!(
        queue_error_seen,
        "expected persistence queue pressure to surface as error"
    );

    handle.shutdown().await.expect("shutdown");
    assert!(!seen.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn durable_ack_mode_waits_for_persistence_before_returning() {
    let sink = SlowSink {
        seen: Arc::new(Mutex::new(Vec::new())),
        delay: Duration::from_millis(150),
    };

    let cfg = RuntimeConfig {
        ack_mode: AckMode::Durable,
        flush_on_insert: false,
        batch_max_ops: 64,
        batch_max_latency_ms: 5_000,
        persist_queue_bound: 8,
        snapshot_every_ops: 0,
        compact_after_snapshot: false,
    };

    let handle = spawn_qsolog(QsoStore::new(), Some(Box::new(sink)), cfg);
    let start = tokio::time::Instant::now();
    let _id = handle.insert(draft("K9DUR", 100)).await.expect("insert");
    let elapsed = start.elapsed();

    assert!(
        elapsed >= Duration::from_millis(120),
        "durable ack should wait for sink flush; elapsed={elapsed:?}"
    );

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn in_memory_ack_emits_persistence_error_and_warning_when_unhealthy() {
    let sink = FailingSink {
        append_calls: 0,
        fail_after: 1,
    };
    let cfg = RuntimeConfig {
        ack_mode: AckMode::InMemory,
        flush_on_insert: true,
        batch_max_ops: 1,
        batch_max_latency_ms: 1,
        persist_queue_bound: 16,
        snapshot_every_ops: 0,
        compact_after_snapshot: false,
    };
    let handle = spawn_qsolog(QsoStore::new(), Some(Box::new(sink)), cfg);
    let mut sub = handle.subscribe();

    let _ = handle.insert(draft("A1", 1)).await.expect("insert1");
    let _ = handle.insert(draft("A2", 2)).await.expect("insert2");

    let mut persistence_error_seen = false;
    for _ in 0..10 {
        let evt = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("recv")
            .expect("event");
        if matches!(evt, QsoEvent::PersistenceError { .. }) {
            persistence_error_seen = true;
            break;
        }
    }
    assert!(persistence_error_seen, "expected persistence error event");

    let state = handle.persistence_state().await;
    assert!(!state.is_healthy);
    assert!(state.last_error.is_some());

    let _ = handle.insert(draft("A3", 3)).await.expect("insert3");
    let mut warning_seen = false;
    for _ in 0..10 {
        let evt = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("recv")
            .expect("event");
        if matches!(evt, QsoEvent::NotDurableWarning { .. }) {
            warning_seen = true;
            break;
        }
    }
    assert!(warning_seen, "expected non-durable warning event");

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn durable_ack_rejects_mutations_while_unhealthy() {
    let sink = FailingSink {
        append_calls: 0,
        fail_after: 1,
    };
    let cfg = RuntimeConfig {
        ack_mode: AckMode::Durable,
        flush_on_insert: true,
        batch_max_ops: 1,
        batch_max_latency_ms: 1,
        persist_queue_bound: 16,
        snapshot_every_ops: 0,
        compact_after_snapshot: false,
    };
    let handle = spawn_qsolog(QsoStore::new(), Some(Box::new(sink)), cfg);
    let mut sub = handle.subscribe();

    let _ = handle.insert(draft("D1", 1)).await.expect("insert1");
    let _ = handle.insert(draft("D2", 2)).await.expect("insert2");

    let mut persistence_error_seen = false;
    for _ in 0..10 {
        let evt = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("recv")
            .expect("event");
        if matches!(evt, QsoEvent::PersistenceError { .. }) {
            persistence_error_seen = true;
            break;
        }
    }
    assert!(persistence_error_seen, "expected persistence error event");

    let err = handle
        .insert(draft("D3", 3))
        .await
        .expect_err("insert should fail");
    assert!(matches!(err, RuntimeError::PersistenceUnhealthy(_)));

    let state = handle.persistence_state().await;
    assert!(!state.is_healthy);
    assert!(state.last_error.is_some());

    handle.shutdown().await.expect("shutdown");
}
