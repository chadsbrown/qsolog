//! Authoritative in-memory QSO logging with append-only SQLite journaling.
//!
//! # Examples
//!
//! In-memory usage with [`core::store::QsoStore`]:
//! ```
//! use qsolog::{
//!     core::store::QsoStore,
//!     qso::{ExchangeBlob, QsoDraft, QsoFlags},
//!     types::{Band, Mode},
//! };
//!
//! let mut store = QsoStore::new();
//! let (id, _op) = store.insert(QsoDraft {
//!     contest_instance_id: 1,
//!     callsign_raw: "K1ABC".to_string(),
//!     callsign_norm: "K1ABC".to_string(),
//!     band: Band::B20m,
//!     mode: Mode::CW,
//!     freq_hz: 14_025_000,
//!     ts_ms: 1,
//!     radio_id: 1,
//!     operator_id: 1,
//!     exchange: ExchangeBlob { bytes: vec![] },
//!     flags: QsoFlags::default(),
//! }).expect("insert");
//! assert_eq!(id, 1);
//! ```
//!
//! Runtime usage with SQLite sink:
//! ```no_run
//! use qsolog::{
//!     core::store::QsoStore,
//!     persist::sqlite::SqliteOpSink,
//!     qso::{ExchangeBlob, QsoDraft, QsoFlags},
//!     runtime::handle::{spawn_qsolog, AckMode, RuntimeConfig},
//!     types::{Band, Mode},
//! };
//!
//! # #[tokio::main]
//! # async fn main() {
//! let sink = SqliteOpSink::open("qsolog.db").expect("open sqlite");
//! let cfg = RuntimeConfig { ack_mode: AckMode::InMemory, ..RuntimeConfig::default() };
//! let handle = spawn_qsolog(QsoStore::new(), Some(Box::new(sink)), cfg);
//! let _id = handle.insert(QsoDraft {
//!     contest_instance_id: 1,
//!     callsign_raw: "K1ABC".to_string(),
//!     callsign_norm: "K1ABC".to_string(),
//!     band: Band::B20m,
//!     mode: Mode::CW,
//!     freq_hz: 14_025_000,
//!     ts_ms: 1,
//!     radio_id: 1,
//!     operator_id: 1,
//!     exchange: ExchangeBlob { bytes: vec![] },
//!     flags: QsoFlags::default(),
//! }).await.expect("insert");
//! handle.shutdown().await.expect("shutdown");
//! # }
//! ```
#![deny(missing_docs)]

/// Core in-memory store and index helpers.
pub mod core;
/// Contest-engine traits and incremental projector.
pub mod engine;
/// Mutation op model and persistence wrapper types.
pub mod op;
/// Persistence abstraction and SQLite implementation.
pub mod persist;
/// QSO domain records and patches.
pub mod qso;
/// Single-writer runtime handle and events.
pub mod runtime;
/// Shared primitive types and enums.
pub mod types;
