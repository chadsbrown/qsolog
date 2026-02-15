use std::sync::Arc;

use tokio::{
    sync::{broadcast, mpsc, oneshot, Mutex},
    time::{Duration, Instant},
};

use crate::{
    core::store::{QsoStore, StoreError},
    op::{Op, StoredOp},
    persist::{OpSink, PersistError},
    qso::{QsoDraft, QsoPatch, QsoRecord},
    types::OpSeq,
};

use super::events::QsoEvent;

#[derive(Debug)]
pub enum RuntimeError {
    Store(StoreError),
    Persist(PersistError),
    ChannelClosed,
}

impl From<StoreError> for RuntimeError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<PersistError> for RuntimeError {
    fn from(value: PersistError) -> Self {
        Self::Persist(value)
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub flush_on_insert: bool,
    pub batch_max_ops: usize,
    pub batch_max_latency_ms: u64,
    pub persist_queue_bound: usize,
    pub snapshot_every_ops: usize,
    pub compact_after_snapshot: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            flush_on_insert: true,
            batch_max_ops: 32,
            batch_max_latency_ms: 75,
            persist_queue_bound: 64,
            snapshot_every_ops: 2000,
            compact_after_snapshot: false,
        }
    }
}

pub struct QsoLogHandle {
    cmd_tx: mpsc::Sender<Command>,
    events_tx: broadcast::Sender<QsoEvent>,
}

impl Clone for QsoLogHandle {
    fn clone(&self) -> Self {
        Self {
            cmd_tx: self.cmd_tx.clone(),
            events_tx: self.events_tx.clone(),
        }
    }
}

enum Command {
    Insert {
        draft: QsoDraft,
        resp: oneshot::Sender<Result<crate::types::QsoId, RuntimeError>>,
    },
    Patch {
        id: crate::types::QsoId,
        patch: QsoPatch,
        resp: oneshot::Sender<Result<(), RuntimeError>>,
    },
    Void {
        id: crate::types::QsoId,
        resp: oneshot::Sender<Result<(), RuntimeError>>,
    },
    Undo {
        resp: oneshot::Sender<Result<(), RuntimeError>>,
    },
    Redo {
        resp: oneshot::Sender<Result<(), RuntimeError>>,
    },
    Get {
        id: crate::types::QsoId,
        resp: oneshot::Sender<Option<QsoRecord>>,
    },
    Recent {
        n: usize,
        resp: oneshot::Sender<Vec<QsoRecord>>,
    },
    ByCall {
        call: String,
        resp: oneshot::Sender<Vec<QsoRecord>>,
    },
    Flush {
        resp: oneshot::Sender<Result<OpSeq, RuntimeError>>,
    },
    Checkpoint {
        resp: oneshot::Sender<Result<(), RuntimeError>>,
    },
    Shutdown {
        resp: oneshot::Sender<Result<(), RuntimeError>>,
    },
}

enum PersistMsg {
    Op(StoredOp),
    Flush {
        resp: oneshot::Sender<Result<OpSeq, PersistError>>,
    },
    Checkpoint {
        snapshot: crate::core::store::StoreSnapshotV1,
        last_seq: OpSeq,
        compact: bool,
        resp: oneshot::Sender<Result<(), PersistError>>,
    },
    Shutdown {
        resp: oneshot::Sender<()>,
    },
}

pub fn spawn_qsolog(
    store: QsoStore,
    sink: Option<Box<dyn OpSink>>,
    config: RuntimeConfig,
) -> QsoLogHandle {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<Command>(256);
    let (events_tx, _) = broadcast::channel::<QsoEvent>(1024);

    let (persist_tx_opt, mut durable_rx) = if let Some(sink) = sink {
        let (persist_tx, persist_rx) = mpsc::channel::<PersistMsg>(config.persist_queue_bound);
        let (durable_tx, durable_rx) = mpsc::unbounded_channel::<Result<OpSeq, PersistError>>();
        spawn_persistence_worker(sink, persist_rx, durable_tx, config.clone());
        (Some(persist_tx), Some(durable_rx))
    } else {
        (None, None)
    };

    let events_tx_loop = events_tx.clone();

    tokio::spawn(async move {
        let mut store = store;
        let mut ops_since_snapshot = 0usize;

        loop {
            if let Some(rx) = durable_rx.as_mut() {
                tokio::select! {
                    cmd = cmd_rx.recv() => {
                        let Some(cmd) = cmd else { break; };
                        let done = handle_command(
                            cmd,
                            &mut store,
                            &events_tx_loop,
                            persist_tx_opt.as_ref(),
                            &config,
                            &mut ops_since_snapshot,
                        ).await;

                        if done {
                            break;
                        }
                    }
                    durable = rx.recv() => {
                        if let Some(Ok(op_seq)) = durable {
                            let _ = events_tx_loop.send(QsoEvent::DurableUpTo { op_seq });
                        }
                    }
                }
            } else {
                let Some(cmd) = cmd_rx.recv().await else { break; };
                let done = handle_command(
                    cmd,
                    &mut store,
                    &events_tx_loop,
                    persist_tx_opt.as_ref(),
                    &config,
                    &mut ops_since_snapshot,
                ).await;
                if done {
                    break;
                }
            }
        }
    });

    QsoLogHandle {
        cmd_tx,
        events_tx,
    }
}

impl QsoLogHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<QsoEvent> {
        self.events_tx.subscribe()
    }

    pub async fn insert(&self, draft: QsoDraft) -> Result<crate::types::QsoId, RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Insert { draft, resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    pub async fn patch(&self, id: crate::types::QsoId, patch: QsoPatch) -> Result<(), RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Patch { id, patch, resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    pub async fn void(&self, id: crate::types::QsoId) -> Result<(), RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Void { id, resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    pub async fn undo(&self) -> Result<(), RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Undo { resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    pub async fn redo(&self) -> Result<(), RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Redo { resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    pub async fn get(&self, id: crate::types::QsoId) -> Result<Option<QsoRecord>, RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Get { id, resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)
    }

    pub async fn recent(&self, n: usize) -> Result<Vec<QsoRecord>, RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Recent { n, resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)
    }

    pub async fn by_call(&self, call: impl Into<String>) -> Result<Vec<QsoRecord>, RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::ByCall {
                call: call.into(),
                resp: tx,
            })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)
    }

    pub async fn flush(&self) -> Result<OpSeq, RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Flush { resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    pub async fn checkpoint(&self) -> Result<(), RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Checkpoint { resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    pub async fn shutdown(&self) -> Result<(), RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::Shutdown { resp: tx })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)?;
        rx.await.map_err(|_| RuntimeError::ChannelClosed)?
    }
}

async fn handle_command(
    cmd: Command,
    store: &mut QsoStore,
    events_tx: &broadcast::Sender<QsoEvent>,
    persist_tx: Option<&mpsc::Sender<PersistMsg>>,
    config: &RuntimeConfig,
    ops_since_snapshot: &mut usize,
) -> bool {
    match cmd {
        Command::Insert { draft, resp } => {
            let res = store
                .insert(draft)
                .map_err(RuntimeError::from)
                .and_then(|(id, stored)| {
                    let out = id;
                    if let Some(tx) = persist_tx {
                        enqueue_persist(tx, stored)?;
                    } else {
                        let _ = events_tx.send(QsoEvent::DurableUpTo {
                            op_seq: store.latest_op_seq(),
                        });
                    }
                    let _ = events_tx.send(QsoEvent::Inserted { id });
                    Ok(out)
                });
            if res.is_ok() {
                *ops_since_snapshot += 1;
                maybe_auto_checkpoint(store, persist_tx, config, ops_since_snapshot).await;
            }
            let _ = resp.send(res);
        }
        Command::Patch { id, patch, resp } => {
            let res = store
                .patch(id, patch)
                .map_err(RuntimeError::from)
                .and_then(|(_, stored)| {
                    if let Some(tx) = persist_tx {
                        enqueue_persist(tx, stored)?;
                    } else {
                        let _ = events_tx.send(QsoEvent::DurableUpTo {
                            op_seq: store.latest_op_seq(),
                        });
                    }
                    let _ = events_tx.send(QsoEvent::Updated { id });
                    Ok(())
                });
            let _ = resp.send(res);
        }
        Command::Void { id, resp } => {
            let res = store
                .void(id)
                .map_err(RuntimeError::from)
                .and_then(|(_, stored)| {
                    if let Some(tx) = persist_tx {
                        enqueue_persist(tx, stored)?;
                    } else {
                        let _ = events_tx.send(QsoEvent::DurableUpTo {
                            op_seq: store.latest_op_seq(),
                        });
                    }
                    let _ = events_tx.send(QsoEvent::Voided { id });
                    Ok(())
                });
            let _ = resp.send(res);
        }
        Command::Undo { resp } => {
            let res = store
                .undo()
                .map_err(RuntimeError::from)
                .and_then(|(_, stored)| {
                    if let Some(tx) = persist_tx {
                        enqueue_persist(tx, stored)?;
                    } else {
                        let _ = events_tx.send(QsoEvent::DurableUpTo {
                            op_seq: store.latest_op_seq(),
                        });
                    }
                    let _ = events_tx.send(QsoEvent::UndoApplied);
                    Ok(())
                });
            let _ = resp.send(res);
        }
        Command::Redo { resp } => {
            let res = store
                .redo()
                .map_err(RuntimeError::from)
                .and_then(|(_, stored)| {
                    if let Some(tx) = persist_tx {
                        enqueue_persist(tx, stored)?;
                    } else {
                        let _ = events_tx.send(QsoEvent::DurableUpTo {
                            op_seq: store.latest_op_seq(),
                        });
                    }
                    let _ = events_tx.send(QsoEvent::RedoApplied);
                    Ok(())
                });
            let _ = resp.send(res);
        }
        Command::Get { id, resp } => {
            let _ = resp.send(store.get_cloned(id));
        }
        Command::Recent { n, resp } => {
            let _ = resp.send(store.recent_cloned(n));
        }
        Command::ByCall { call, resp } => {
            let _ = resp.send(store.by_call_cloned(&call));
        }
        Command::Flush { resp } => {
            let out = if let Some(tx) = persist_tx {
                let (flush_tx, flush_rx) = oneshot::channel();
                if tx
                    .send(PersistMsg::Flush { resp: flush_tx })
                    .await
                    .is_err()
                {
                    Err(RuntimeError::ChannelClosed)
                } else {
                    flush_rx
                        .await
                        .map_err(|_| RuntimeError::ChannelClosed)
                        .and_then(|r| r.map_err(RuntimeError::from))
                }
            } else {
                Ok(store.latest_op_seq())
            };
            let _ = resp.send(out);
        }
        Command::Checkpoint { resp } => {
            let out = if let Some(tx) = persist_tx {
                let snapshot = store.export_snapshot();
                let last_seq = store.latest_op_seq();
                let (cp_tx, cp_rx) = oneshot::channel();
                if tx
                    .send(PersistMsg::Checkpoint {
                        snapshot,
                        last_seq,
                        compact: config.compact_after_snapshot,
                        resp: cp_tx,
                    })
                    .await
                    .is_err()
                {
                    Err(RuntimeError::ChannelClosed)
                } else {
                    cp_rx
                        .await
                        .map_err(|_| RuntimeError::ChannelClosed)
                        .and_then(|r| r.map_err(RuntimeError::from))
                }
            } else {
                Ok(())
            };
            let _ = resp.send(out);
        }
        Command::Shutdown { resp } => {
            let out = if let Some(tx) = persist_tx {
                let (done_tx, done_rx) = oneshot::channel();
                let send_res = tx.send(PersistMsg::Shutdown { resp: done_tx }).await;
                if send_res.is_err() {
                    Err(RuntimeError::ChannelClosed)
                } else {
                    match done_rx.await {
                        Ok(()) => Ok(()),
                        Err(_) => Err(RuntimeError::ChannelClosed),
                    }
                }
            } else {
                Ok(())
            };
            let _ = resp.send(out);
            return true;
        }
    }

    false
}

fn spawn_persistence_worker(
    sink: Box<dyn OpSink>,
    mut rx: mpsc::Receiver<PersistMsg>,
    durable_tx: mpsc::UnboundedSender<Result<OpSeq, PersistError>>,
    config: RuntimeConfig,
) {
    let sink = Arc::new(Mutex::new(sink));
    tokio::spawn(async move {
        let mut buf = Vec::<StoredOp>::new();
        let mut deadline = Instant::now() + Duration::from_millis(config.batch_max_latency_ms);
        let mut last_durable: OpSeq = 0;

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else {
                        let _ = flush_buf(&sink, &mut buf, &mut last_durable, &durable_tx, true).await;
                        break;
                    };

                    match msg {
                        PersistMsg::Op(stored) => {
                            let is_insert = matches!(stored.op, Op::Insert { .. });
                            buf.push(stored);

                            if buf.len() >= config.batch_max_ops || (config.flush_on_insert && is_insert) {
                                let _ = flush_buf(&sink, &mut buf, &mut last_durable, &durable_tx, true).await;
                                deadline = Instant::now() + Duration::from_millis(config.batch_max_latency_ms);
                            }
                        }
                        PersistMsg::Flush { resp } => {
                            let result = flush_buf(&sink, &mut buf, &mut last_durable, &durable_tx, true).await;
                            let _ = resp.send(result.map(|_| last_durable));
                            deadline = Instant::now() + Duration::from_millis(config.batch_max_latency_ms);
                        }
                        PersistMsg::Checkpoint { snapshot, last_seq, compact, resp } => {
                            let flush_result = flush_buf(&sink, &mut buf, &mut last_durable, &durable_tx, true).await;
                            let result = if let Err(err) = flush_result {
                                Err(err)
                            } else {
                                let sink_ref = Arc::clone(&sink);
                                match tokio::task::spawn_blocking(move || {
                                    let mut sink = sink_ref.blocking_lock();
                                    sink.write_snapshot(&snapshot, last_seq)?;
                                    if compact {
                                        let _ = sink.compact_through(last_seq)?;
                                    }
                                    Result::<(), PersistError>::Ok(())
                                }).await {
                                    Ok(inner) => inner,
                                    Err(e) => Err(PersistError::Message(format!("join error: {e}"))),
                                }
                            };
                            let _ = resp.send(result);
                            deadline = Instant::now() + Duration::from_millis(config.batch_max_latency_ms);
                        }
                        PersistMsg::Shutdown { resp } => {
                            let _ = flush_buf(&sink, &mut buf, &mut last_durable, &durable_tx, true).await;
                            let _ = resp.send(());
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline), if !buf.is_empty() => {
                    let _ = flush_buf(&sink, &mut buf, &mut last_durable, &durable_tx, false).await;
                    deadline = Instant::now() + Duration::from_millis(config.batch_max_latency_ms);
                }
            }
        }
    });
}

async fn flush_buf(
    sink: &Arc<Mutex<Box<dyn OpSink>>>,
    buf: &mut Vec<StoredOp>,
    last_durable: &mut OpSeq,
    durable_tx: &mpsc::UnboundedSender<Result<OpSeq, PersistError>>,
    call_flush: bool,
) -> Result<(), PersistError> {
    if buf.is_empty() {
        if call_flush {
            let sink_ref = Arc::clone(sink);
            tokio::task::spawn_blocking(move || {
                let mut sink = sink_ref.blocking_lock();
                sink.flush()
            })
            .await
            .map_err(|e| PersistError::Message(format!("join error: {e}")))??;
        }
        return Ok(());
    }

    let ops = std::mem::take(buf);
    let sink_ref = Arc::clone(sink);
    let append_res: Result<OpSeq, PersistError> = tokio::task::spawn_blocking(move || {
        let mut sink = sink_ref.blocking_lock();
        let seq = sink.append_ops(&ops)?;
        if call_flush {
            sink.flush()?;
        }
        Ok(seq)
    })
    .await
    .map_err(|e| PersistError::Message(format!("join error: {e}")))?;

    match append_res {
        Ok(seq) => {
            *last_durable = (*last_durable).max(seq);
            let _ = durable_tx.send(Ok(*last_durable));
            Ok(())
        }
        Err(err) => {
            let _ = durable_tx.send(Err(PersistError::Message(format!("append failed: {err:?}"))));
            Err(err)
        }
    }
}

async fn maybe_auto_checkpoint(
    store: &QsoStore,
    persist_tx: Option<&mpsc::Sender<PersistMsg>>,
    config: &RuntimeConfig,
    ops_since_snapshot: &mut usize,
) {
    if config.snapshot_every_ops == 0 || *ops_since_snapshot < config.snapshot_every_ops {
        return;
    }

    let Some(tx) = persist_tx else {
        return;
    };

    let snapshot = store.export_snapshot();
    let last_seq = store.latest_op_seq();
    let (cp_tx, cp_rx) = oneshot::channel();
    if tx
        .send(PersistMsg::Checkpoint {
            snapshot,
            last_seq,
            compact: config.compact_after_snapshot,
            resp: cp_tx,
        })
        .await
        .is_ok()
    {
        let _ = cp_rx.await;
        *ops_since_snapshot = 0;
    }
}

fn enqueue_persist(tx: &mpsc::Sender<PersistMsg>, stored: StoredOp) -> Result<(), RuntimeError> {
    tx.try_send(PersistMsg::Op(stored))
        .map_err(|err| RuntimeError::Persist(PersistError::Message(format!("persist queue error: {err}"))))
}
