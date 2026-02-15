# qsolog

`qsolog` is a Rust crate for contest-style QSO logging with:

- authoritative in-memory state
- append-only operation journaling to SQLite
- deterministic insertion-order semantics
- undo/redo via compensating operations
- single-writer async runtime with event subscription

## Status

The crate currently includes:

- core domain types (`QsoRecord`, `QsoPatch`, `Op`, `StoredOp`)
- in-memory `QsoStore` with stable canonical order
- Tokio runtime handle and command loop (`spawn_qsolog`)
- persistence worker with batching + durability progress events
- SQLite sink with WAL mode and snapshot/replay support
- incremental contest-engine projector interface and test engine
- integration tests + property tests + throughput benchmarks

## Non-Negotiable Invariants

- Canonical order is insertion order and never reorders.
- QSO identity is monotonic `u64`.
- Persistence is append-only op journal (+ optional snapshots).
- Undo/redo is modeled as compensating operations.
- Runtime API is single-writer through a command handle.

## Crate Layout

- `src/types.rs`: primitive/shared IDs and enums
- `src/qso.rs`: QSO records, drafts, patches
- `src/op.rs`: operation and stored-operation types
- `src/core/store.rs`: authoritative in-memory store
- `src/runtime/handle.rs`: async command runtime and persistence worker bridge
- `src/runtime/events.rs`: event stream types
- `src/persist/sqlite.rs`: SQLite op sink, replay, snapshots
- `src/engine/traits.rs`: contest-engine abstraction
- `src/engine/projector.rs`: incremental invalidation projector

## Durability Semantics

`RuntimeConfig` controls acknowledgment policy:

- `AckMode::InMemory` (default): mutating commands succeed after in-memory apply + persistence queueing.
- `AckMode::Durable`: mutating commands wait until persistence flush completes.

Durability progress is emitted via:

- `QsoEvent::DurableUpTo { op_seq }`

Backpressure policy is explicit:

- bounded persistence queue uses **error-on-full** semantics (`RuntimeError::PersistQueueFull`)
- no silent dropping of operations

## SQLite Notes

On connection open, the sink sets:

- `PRAGMA journal_mode=WAL;`
- `PRAGMA synchronous=NORMAL;`

Operations are written in transactions with prepared statements.

## Quick Start

```rust
use qsolog::{
    core::store::QsoStore,
    persist::sqlite::SqliteOpSink,
    qso::{ExchangeBlob, QsoDraft, QsoFlags},
    runtime::handle::{spawn_qsolog, AckMode, RuntimeConfig},
    types::{Band, Mode},
};

#[tokio::main]
async fn main() {
    let store = QsoStore::new();
    let sink = SqliteOpSink::open("qsolog.db").expect("open sqlite");

    let cfg = RuntimeConfig {
        ack_mode: AckMode::InMemory,
        ..RuntimeConfig::default()
    };

    let handle = spawn_qsolog(store, Some(Box::new(sink)), cfg);

    let mut events = handle.subscribe();

    let id = handle.insert(QsoDraft {
        contest_instance_id: 1,
        callsign_raw: "K1ABC".to_string(),
        callsign_norm: "K1ABC".to_string(),
        band: Band::B20m,
        mode: Mode::CW,
        freq_hz: 14_025_000,
        ts_ms: 0,
        radio_id: 1,
        operator_id: 1,
        exchange: ExchangeBlob { bytes: vec![] },
        flags: QsoFlags::default(),
    }).await.expect("insert");

    println!("inserted id={id}");

    // Drain an event (example)
    let _evt = events.recv().await.expect("event");

    handle.shutdown().await.expect("shutdown");
}
```

## Testing and Benchmarks

- Run tests: `cargo test`
- Run throughput bench: `cargo bench --bench throughput`

## Current Gaps

The API and internals are working, but if you plan to stabilize this as a public crate, next steps are:

- rustdoc on all public APIs
- explicit persistence failure-state policy surfaced in handle APIs
- finalized on-disk compatibility/version migration policy
