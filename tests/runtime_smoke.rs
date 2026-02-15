use std::{sync::{Arc, Mutex}, time::Duration};

use qsolog::{
    core::store::QsoStore,
    persist::OpSink,
    qso::{ExchangeBlob, QsoDraft, QsoFlags, QsoPatch},
    runtime::{
        events::QsoEvent,
        handle::{spawn_qsolog, RuntimeConfig, RuntimeError},
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
    fn append_ops(&mut self, ops: &[qsolog::op::StoredOp]) -> qsolog::persist::PersistResult<OpSeq> {
        std::thread::sleep(self.delay);
        let mut seen = self.seen.lock().expect("lock");
        for op in ops {
            seen.push(op.seq);
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
        if let Err(RuntimeError::Persist(_)) = r {
            queue_error_seen = true;
            break;
        }
    }
    assert!(queue_error_seen, "expected persistence queue pressure to surface as error");

    handle.shutdown().await.expect("shutdown");
    assert!(!seen.lock().expect("lock").is_empty());
}
