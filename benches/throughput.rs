use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use qsolog::{
    core::store::QsoStore,
    qso::{ExchangeBlob, QsoDraft, QsoFlags, QsoPatch},
    types::{Band, Mode},
};

fn draft(call: &str, ts: u64) -> QsoDraft {
    QsoDraft {
        contest_instance_id: 1,
        callsign_raw: call.to_string(),
        callsign_norm: call.to_string(),
        band: Band::B20m,
        mode: Mode::CW,
        freq_hz: 14_050_000,
        ts_ms: ts,
        radio_id: 1,
        operator_id: 1,
        exchange: ExchangeBlob { bytes: vec![] },
        flags: QsoFlags::default(),
    }
}

fn bench_inserts(c: &mut Criterion) {
    c.bench_function("store_insert_50k", |b| {
        b.iter(|| {
            let mut store = QsoStore::new();
            for i in 0..50_000u64 {
                let _ = store.insert(draft(&format!("K{i}"), i)).expect("insert");
            }
        });
    });
}

fn bench_random_patches(c: &mut Criterion) {
    c.bench_function("store_patch_10k", |b| {
        b.iter(|| {
            let mut store = QsoStore::new();
            for i in 0..10_000u64 {
                let _ = store.insert(draft(&format!("W{i}"), i)).expect("insert");
            }
            for i in 0..10_000u64 {
                let _ = store
                    .patch(
                        i + 1,
                        QsoPatch {
                            freq_hz: Some(14_000_000 + i),
                            ..QsoPatch::default()
                        },
                    )
                    .expect("patch");
            }
        });
    });
}

fn bench_recent_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("recent_query");
    let mut store = QsoStore::new();
    for i in 0..50_000u64 {
        let _ = store.insert(draft(&format!("N{i}"), i)).expect("insert");
    }

    for n in [10usize, 100usize, 1000usize] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                let _ = store.recent(n);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_inserts, bench_random_patches, bench_recent_query);
criterion_main!(benches);
