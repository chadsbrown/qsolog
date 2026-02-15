//! Authoritative in-memory QSO store.
//!
//! Invariants:
//! - insertion order is canonical and never reorders
//! - every mutating API emits a [`StoredOp`] for journaling
//! - undo/redo are implemented via compensating operations

use std::time::{SystemTime, UNIX_EPOCH};

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

use crate::{
    op::{Op, StoredOp},
    qso::{QsoDraft, QsoPatch, QsoRecord},
    types::{ContestInstanceId, OpSeq, QsoId},
};

/// Error type for in-memory store operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Referenced QSO id does not exist.
    MissingQso(QsoId),
    /// Insert attempted for an id that already exists.
    AlreadyExists(QsoId),
    /// Undo stack is empty.
    NothingToUndo,
    /// Redo stack is empty.
    NothingToRedo,
}

/// Serializable store snapshot for checkpointing/replay bootstrap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreSnapshotV1 {
    /// Next ID to assign on insert.
    pub next_qso_id: QsoId,
    /// Next op sequence to assign.
    pub next_op_seq: OpSeq,
    /// Canonical insertion-order id list.
    pub order: Vec<QsoId>,
    /// Record set.
    pub records: Vec<QsoRecord>,
}

/// Authoritative mutable QSO store.
#[derive(Debug, Default)]
pub struct QsoStore {
    records: HashMap<QsoId, QsoRecord>,
    order: Vec<QsoId>,
    pos: HashMap<QsoId, usize>,
    by_call: HashMap<String, Vec<QsoId>>,
    by_contest: HashMap<ContestInstanceId, Vec<QsoId>>,
    undo: Vec<Op>,
    redo: Vec<Op>,
    pending_ops: Vec<StoredOp>,
    next_op_seq: OpSeq,
    next_qso_id: QsoId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MutationCheckpoint {
    undo_len: usize,
    redo_len: usize,
    pending_ops_len: usize,
    next_op_seq: OpSeq,
    next_qso_id: QsoId,
}

impl QsoStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self {
            next_op_seq: 1,
            next_qso_id: 1,
            ..Self::default()
        }
    }

    /// Restores store state from a snapshot.
    pub fn from_snapshot(snapshot: StoreSnapshotV1) -> Result<Self, StoreError> {
        let mut store = Self {
            next_qso_id: snapshot.next_qso_id,
            next_op_seq: snapshot.next_op_seq,
            order: snapshot.order,
            ..Self::default()
        };

        for (idx, id) in store.order.iter().copied().enumerate() {
            store.pos.insert(id, idx);
        }

        for rec in snapshot.records {
            store.insert_indices(&rec);
            store.records.insert(rec.id, rec);
        }

        Ok(store)
    }

    /// Exports a snapshot suitable for persistence.
    pub fn export_snapshot(&self) -> StoreSnapshotV1 {
        let records = self
            .order
            .iter()
            .filter_map(|id| self.records.get(id).cloned())
            .collect();

        StoreSnapshotV1 {
            next_qso_id: self.next_qso_id,
            next_op_seq: self.next_op_seq,
            order: self.order.clone(),
            records,
        }
    }

    /// Inserts a new QSO and returns `(id, stored_op)`.
    pub fn insert(&mut self, draft: QsoDraft) -> Result<(QsoId, StoredOp), StoreError> {
        let id = self.next_qso_id;
        self.next_qso_id += 1;

        let qso = QsoRecord {
            id,
            contest_instance_id: draft.contest_instance_id,
            callsign_raw: draft.callsign_raw,
            callsign_norm: draft.callsign_norm,
            band: draft.band,
            mode: draft.mode,
            freq_hz: draft.freq_hz,
            ts_ms: draft.ts_ms,
            radio_id: draft.radio_id,
            operator_id: draft.operator_id,
            exchange: draft.exchange,
            flags: draft.flags,
        };

        let (stored, inverse) = self.apply_insert(qso)?;
        self.undo.push(inverse);
        self.redo.clear();
        self.pending_ops.push(stored.clone());
        Ok((id, stored))
    }

    /// Applies a patch to an existing QSO and returns the emitted op.
    pub fn patch(&mut self, id: QsoId, patch: QsoPatch) -> Result<((), StoredOp), StoreError> {
        let (stored, inverse) = self.apply_patch(id, patch)?;
        self.undo.push(inverse);
        self.redo.clear();
        self.pending_ops.push(stored.clone());
        Ok(((), stored))
    }

    /// Toggles void status for a QSO and returns the emitted op.
    pub fn void(&mut self, id: QsoId) -> Result<((), StoredOp), StoreError> {
        let prev_is_void = self
            .records
            .get(&id)
            .ok_or(StoreError::MissingQso(id))?
            .flags
            .is_void;
        let (stored, inverse) = self.apply_void(id, prev_is_void)?;
        self.undo.push(inverse);
        self.redo.clear();
        self.pending_ops.push(stored.clone());
        Ok(((), stored))
    }

    /// Applies one undo step and returns the compensating op.
    pub fn undo(&mut self) -> Result<((), StoredOp), StoreError> {
        let op = self.undo.pop().ok_or(StoreError::NothingToUndo)?;
        let (stored, inverse) = self.apply_op(op)?;
        self.redo.push(inverse);
        self.pending_ops.push(stored.clone());
        Ok(((), stored))
    }

    /// Applies one redo step and returns the compensating op.
    pub fn redo(&mut self) -> Result<((), StoredOp), StoreError> {
        let op = self.redo.pop().ok_or(StoreError::NothingToRedo)?;
        let (stored, inverse) = self.apply_op(op)?;
        self.undo.push(inverse);
        self.pending_ops.push(stored.clone());
        Ok(((), stored))
    }

    /// Applies a stored operation during journal replay.
    ///
    /// Replay mode intentionally clears undo/redo stacks.
    pub fn apply_replayed_op(&mut self, stored: StoredOp) -> Result<(), StoreError> {
        let seq = stored.seq;
        let op = stored.op;
        match op {
            Op::Insert { qso } => {
                self.apply_insert_with_seq(qso, seq)?;
            }
            Op::Patch { id, patch, .. } => {
                self.apply_patch_with_seq(id, patch, seq)?;
            }
            Op::Void { id, prev_is_void } => {
                self.apply_void_with_seq(id, prev_is_void, seq)?;
            }
        }
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }

    /// Returns a record reference by id.
    pub fn get(&self, id: QsoId) -> Option<&QsoRecord> {
        self.records.get(&id)
    }

    /// Returns a cloned record by id.
    pub fn get_cloned(&self, id: QsoId) -> Option<QsoRecord> {
        self.get(id).cloned()
    }

    /// Returns up to `n` most-recent records in insertion order.
    pub fn recent(&self, n: usize) -> Vec<&QsoRecord> {
        let len = self.order.len();
        let start = len.saturating_sub(n);
        self.order[start..]
            .iter()
            .filter_map(|id| self.records.get(id))
            .collect()
    }

    /// Cloning variant of [`Self::recent`].
    pub fn recent_cloned(&self, n: usize) -> Vec<QsoRecord> {
        self.recent(n).into_iter().cloned().collect()
    }

    /// Returns all records for a normalized callsign in insertion order.
    pub fn by_call(&self, call_norm: &str) -> Vec<&QsoRecord> {
        self.by_call
            .get(call_norm)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.records.get(id))
            .collect()
    }

    /// Cloning variant of [`Self::by_call`].
    pub fn by_call_cloned(&self, call_norm: &str) -> Vec<QsoRecord> {
        self.by_call(call_norm).into_iter().cloned().collect()
    }

    /// Returns canonical insertion-order ids.
    pub fn ordered_ids(&self) -> &[QsoId] {
        &self.order
    }

    /// Drains pending emitted ops for persistence.
    pub fn drain_pending_ops(&mut self) -> Vec<StoredOp> {
        std::mem::take(&mut self.pending_ops)
    }

    /// Clears buffered pending operations without allocating.
    pub fn clear_pending_ops(&mut self) {
        self.pending_ops.clear();
    }

    /// Number of undo entries.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Number of redo entries.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Latest emitted sequence number, or 0 when none.
    pub fn latest_op_seq(&self) -> OpSeq {
        self.next_op_seq.saturating_sub(1)
    }

    pub(crate) fn mutation_checkpoint(&self) -> MutationCheckpoint {
        MutationCheckpoint {
            undo_len: self.undo.len(),
            redo_len: self.redo.len(),
            pending_ops_len: self.pending_ops.len(),
            next_op_seq: self.next_op_seq,
            next_qso_id: self.next_qso_id,
        }
    }

    pub(crate) fn rollback_mutation(
        &mut self,
        checkpoint: MutationCheckpoint,
        stored: &StoredOp,
    ) -> Result<(), StoreError> {
        match &stored.op {
            Op::Insert { qso } => self.rollback_insert(qso.id)?,
            Op::Patch { id, prev, .. } => self.rollback_patch(*id, prev)?,
            Op::Void { id, prev_is_void } => self.rollback_void(*id, *prev_is_void)?,
        }

        self.next_op_seq = checkpoint.next_op_seq;
        self.next_qso_id = checkpoint.next_qso_id;
        self.undo.truncate(checkpoint.undo_len);
        self.redo.truncate(checkpoint.redo_len);
        self.pending_ops.truncate(checkpoint.pending_ops_len);
        Ok(())
    }

    fn apply_op(&mut self, op: Op) -> Result<(StoredOp, Op), StoreError> {
        match op {
            Op::Insert { qso } => self.apply_insert(qso),
            Op::Patch { id, patch, .. } => self.apply_patch(id, patch),
            Op::Void { id, prev_is_void } => self.apply_void(id, prev_is_void),
        }
    }

    fn apply_insert(&mut self, qso: QsoRecord) -> Result<(StoredOp, Op), StoreError> {
        let seq = self.take_next_op_seq();
        self.apply_insert_with_seq(qso, seq)
    }

    fn apply_insert_with_seq(
        &mut self,
        qso: QsoRecord,
        seq: OpSeq,
    ) -> Result<(StoredOp, Op), StoreError> {
        if self.records.contains_key(&qso.id) {
            return Err(StoreError::AlreadyExists(qso.id));
        }

        let id = qso.id;
        self.next_qso_id = self.next_qso_id.max(id.saturating_add(1));
        self.insert_indices(&qso);
        self.pos.insert(id, self.order.len());
        self.order.push(id);
        self.records.insert(id, qso.clone());

        self.bump_next_seq_from(seq);
        let stored = StoredOp {
            seq,
            ts_ms: now_ms(),
            op: Op::Insert { qso },
        };
        let inverse = Op::Void {
            id,
            prev_is_void: false,
        };
        Ok((stored, inverse))
    }

    fn apply_patch(&mut self, id: QsoId, patch: QsoPatch) -> Result<(StoredOp, Op), StoreError> {
        let seq = self.take_next_op_seq();
        self.apply_patch_with_seq(id, patch, seq)
    }

    fn apply_patch_with_seq(
        &mut self,
        id: QsoId,
        patch: QsoPatch,
        seq: OpSeq,
    ) -> Result<(StoredOp, Op), StoreError> {
        let (prev, old_call, old_contest, new_call, new_contest) = {
            let rec = self
                .records
                .get_mut(&id)
                .ok_or(StoreError::MissingQso(id))?;
            let old_call = rec.callsign_norm.clone();
            let old_contest = rec.contest_instance_id;

            let prev = patch.capture_inverse_for(rec);
            patch.apply_to(rec);

            (
                prev,
                old_call,
                old_contest,
                rec.callsign_norm.clone(),
                rec.contest_instance_id,
            )
        };

        if old_call != new_call {
            self.update_call_index(id, &old_call, &new_call);
        }
        if old_contest != new_contest {
            self.update_contest_index(id, old_contest, new_contest);
        }
        #[cfg(debug_assertions)]
        self.debug_assert_indices_consistent();

        self.bump_next_seq_from(seq);
        let stored = StoredOp {
            seq,
            ts_ms: now_ms(),
            op: Op::Patch {
                id,
                patch: patch.clone(),
                prev: prev.clone(),
            },
        };
        let inverse = Op::Patch {
            id,
            patch: prev,
            prev: patch,
        };
        Ok((stored, inverse))
    }

    fn apply_void(&mut self, id: QsoId, prev_is_void: bool) -> Result<(StoredOp, Op), StoreError> {
        let seq = self.take_next_op_seq();
        self.apply_void_with_seq(id, prev_is_void, seq)
    }

    fn apply_void_with_seq(
        &mut self,
        id: QsoId,
        prev_is_void: bool,
        seq: OpSeq,
    ) -> Result<(StoredOp, Op), StoreError> {
        let new_is_void = {
            let rec = self
                .records
                .get_mut(&id)
                .ok_or(StoreError::MissingQso(id))?;
            rec.flags.is_void = !prev_is_void;
            rec.flags.is_void
        };

        self.bump_next_seq_from(seq);
        let stored = StoredOp {
            seq,
            ts_ms: now_ms(),
            op: Op::Void { id, prev_is_void },
        };
        let inverse = Op::Void {
            id,
            prev_is_void: new_is_void,
        };
        Ok((stored, inverse))
    }

    fn insert_indices(&mut self, rec: &QsoRecord) {
        self.by_call
            .entry(rec.callsign_norm.clone())
            .or_default()
            .push(rec.id);
        self.by_contest
            .entry(rec.contest_instance_id)
            .or_default()
            .push(rec.id);
    }

    fn remove_indices(&mut self, rec: &QsoRecord) {
        if let Some(ids) = self.by_call.get_mut(&rec.callsign_norm) {
            ids.retain(|v| *v != rec.id);
            if ids.is_empty() {
                self.by_call.remove(&rec.callsign_norm);
            }
        }
        if let Some(ids) = self.by_contest.get_mut(&rec.contest_instance_id) {
            ids.retain(|v| *v != rec.id);
            if ids.is_empty() {
                self.by_contest.remove(&rec.contest_instance_id);
            }
        }
    }

    fn update_call_index(&mut self, id: QsoId, old_call: &str, new_call: &str) {
        if let Some(ids) = self.by_call.get_mut(old_call) {
            ids.retain(|v| *v != id);
            if ids.is_empty() {
                self.by_call.remove(old_call);
            }
        }
        let ids = self.by_call.entry(new_call.to_string()).or_default();
        Self::insert_id_ordered(ids, id, &self.pos);
    }

    fn update_contest_index(&mut self, id: QsoId, old: ContestInstanceId, new: ContestInstanceId) {
        if let Some(ids) = self.by_contest.get_mut(&old) {
            ids.retain(|v| *v != id);
            if ids.is_empty() {
                self.by_contest.remove(&old);
            }
        }
        let ids = self.by_contest.entry(new).or_default();
        Self::insert_id_ordered(ids, id, &self.pos);
    }

    fn insert_id_ordered(index: &mut Vec<QsoId>, id: QsoId, pos: &HashMap<QsoId, usize>) {
        let target = *pos.get(&id).unwrap_or(&usize::MAX);
        let at = index
            .binary_search_by_key(&target, |existing| {
                *pos.get(existing).unwrap_or(&usize::MAX)
            })
            .unwrap_or_else(|i| i);
        index.insert(at, id);
    }

    #[cfg(debug_assertions)]
    fn debug_assert_indices_consistent(&self) {
        for (call, ids) in &self.by_call {
            let expected: Vec<QsoId> = self
                .order
                .iter()
                .copied()
                .filter(|id| {
                    self.records
                        .get(id)
                        .is_some_and(|r| r.callsign_norm == *call)
                })
                .collect();
            debug_assert_eq!(&expected, ids);
        }
        for (contest, ids) in &self.by_contest {
            let expected: Vec<QsoId> = self
                .order
                .iter()
                .copied()
                .filter(|id| {
                    self.records
                        .get(id)
                        .is_some_and(|r| r.contest_instance_id == *contest)
                })
                .collect();
            debug_assert_eq!(&expected, ids);
        }
    }

    fn take_next_op_seq(&mut self) -> OpSeq {
        let seq = self.next_op_seq;
        self.next_op_seq += 1;
        seq
    }

    fn bump_next_seq_from(&mut self, seq: OpSeq) {
        self.next_op_seq = self.next_op_seq.max(seq.saturating_add(1));
    }

    fn rollback_insert(&mut self, id: QsoId) -> Result<(), StoreError> {
        let rec = self.records.remove(&id).ok_or(StoreError::MissingQso(id))?;
        self.remove_indices(&rec);

        let idx = self.pos.remove(&id).ok_or(StoreError::MissingQso(id))?;
        self.order.remove(idx);
        for (i, qso_id) in self.order.iter().copied().enumerate().skip(idx) {
            self.pos.insert(qso_id, i);
        }

        Ok(())
    }

    fn rollback_patch(&mut self, id: QsoId, prev: &QsoPatch) -> Result<(), StoreError> {
        let (old_call, old_contest, new_call, new_contest) = {
            let rec = self
                .records
                .get_mut(&id)
                .ok_or(StoreError::MissingQso(id))?;
            let old_call = rec.callsign_norm.clone();
            let old_contest = rec.contest_instance_id;
            prev.apply_to(rec);
            (
                old_call,
                old_contest,
                rec.callsign_norm.clone(),
                rec.contest_instance_id,
            )
        };

        if old_call != new_call {
            self.update_call_index(id, &old_call, &new_call);
        }
        if old_contest != new_contest {
            self.update_contest_index(id, old_contest, new_contest);
        }
        #[cfg(debug_assertions)]
        self.debug_assert_indices_consistent();
        Ok(())
    }

    fn rollback_void(&mut self, id: QsoId, prev_is_void: bool) -> Result<(), StoreError> {
        let rec = self
            .records
            .get_mut(&id)
            .ok_or(StoreError::MissingQso(id))?;
        rec.flags.is_void = prev_is_void;
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
