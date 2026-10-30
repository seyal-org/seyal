use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{SyncSender, TrySendError},
    Arc,
};

use seyal_exec::ReactorWaker;

use crate::{ExecutionId, RuntimeError};

pub(crate) enum ControlMessage {
    Input(AcceptedInput),
}

/// Who produced admitted bytes. Composer submissions are the only kind that
/// may start a Block; every other route is direct input and closes the
/// prompt gate at admission (ADR-009 mechanism 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputKind {
    Direct,
    /// Only the macOS composer route constructs this today.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Composer,
}

pub(crate) struct AcceptedInput {
    pub(crate) execution_id: ExecutionId,
    pub(crate) bytes: Vec<u8>,
    pub(crate) offset: usize,
    pub(crate) reservation: InputReservation,
    pub(crate) kind: InputKind,
}

impl AcceptedInput {
    pub(crate) fn remaining(&self) -> &[u8] {
        &self.bytes[self.offset..]
    }

    pub(crate) fn consume(&mut self, count: usize) {
        self.offset += count;
        #[cfg(all(target_os = "macos", feature = "benchmark-instrumentation"))]
        crate::pass7_benchmark::mark_pass7_pty_write(count);
        self.reservation.consume(count);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

pub(crate) struct InputReservation {
    global: Arc<AtomicUsize>,
    per_execution: Arc<AtomicUsize>,
    remaining: usize,
}

impl InputReservation {
    fn reserve(
        global: Arc<AtomicUsize>,
        global_limit: usize,
        per_execution: Arc<AtomicUsize>,
        per_execution_limit: usize,
        amount: usize,
    ) -> Result<Self, RuntimeError> {
        reserve_counter(&global, global_limit, amount)?;
        if let Err(error) = reserve_counter(&per_execution, per_execution_limit, amount) {
            global.fetch_sub(amount, Ordering::AcqRel);
            return Err(error);
        }
        Ok(Self {
            global,
            per_execution,
            remaining: amount,
        })
    }

    pub(crate) fn consume(&mut self, amount: usize) {
        debug_assert!(amount <= self.remaining);
        if amount == 0 {
            return;
        }
        self.remaining -= amount;
        self.global.fetch_sub(amount, Ordering::AcqRel);
        self.per_execution.fetch_sub(amount, Ordering::AcqRel);
    }
}

impl Drop for InputReservation {
    fn drop(&mut self) {
        if self.remaining == 0 {
            return;
        }
        self.global.fetch_sub(self.remaining, Ordering::AcqRel);
        self.per_execution
            .fetch_sub(self.remaining, Ordering::AcqRel);
        self.remaining = 0;
    }
}

fn reserve_counter(counter: &AtomicUsize, limit: usize, amount: usize) -> Result<(), RuntimeError> {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        let Some(next) = current.checked_add(amount) else {
            return Err(RuntimeError::InputBackpressure);
        };
        if next > limit {
            return Err(RuntimeError::InputBackpressure);
        }
        match counter.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}

#[derive(Clone)]
pub struct InputIngress {
    execution_id: ExecutionId,
    active: Arc<AtomicBool>,
    sender: SyncSender<ControlMessage>,
    waker: ReactorWaker,
    global: Arc<AtomicUsize>,
    global_limit: usize,
    per_execution: Arc<AtomicUsize>,
    per_execution_limit: usize,
    /// Set while F4 is Armed or Delivering. One atomic load; not an F9 sample.
    held: Option<Arc<AtomicBool>>,
}

impl InputIngress {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        execution_id: ExecutionId,
        active: Arc<AtomicBool>,
        sender: SyncSender<ControlMessage>,
        waker: ReactorWaker,
        global: Arc<AtomicUsize>,
        global_limit: usize,
        per_execution: Arc<AtomicUsize>,
        per_execution_limit: usize,
        held: Option<Arc<AtomicBool>>,
    ) -> Self {
        Self {
            execution_id,
            active,
            sender,
            waker,
            global,
            global_limit,
            per_execution,
            per_execution_limit,
            held,
        }
    }

    pub fn try_submit(&self, bytes: Vec<u8>) -> Result<(), RuntimeError> {
        self.try_submit_kind(bytes, InputKind::Direct)
    }

    pub(crate) fn try_submit_kind(
        &self,
        bytes: Vec<u8>,
        kind: InputKind,
    ) -> Result<(), RuntimeError> {
        if !self.active.load(Ordering::Acquire) {
            return Err(RuntimeError::ExecutionNotRunning);
        }
        if self
            .held
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            return Err(RuntimeError::InputHeld);
        }
        let reservation = InputReservation::reserve(
            Arc::clone(&self.global),
            self.global_limit,
            Arc::clone(&self.per_execution),
            self.per_execution_limit,
            bytes.len(),
        )?;
        if !self.active.load(Ordering::Acquire) {
            drop(reservation);
            return Err(RuntimeError::ExecutionNotRunning);
        }
        let message = ControlMessage::Input(AcceptedInput {
            execution_id: self.execution_id,
            bytes,
            offset: 0,
            reservation,
            kind,
        });
        match self.sender.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(message)) => {
                drop(message);
                return Err(RuntimeError::ControlQueueFull);
            }
            Err(TrySendError::Disconnected(message)) => {
                drop(message);
                return Err(RuntimeError::ControlQueueClosed);
            }
        }
        #[cfg(all(target_os = "macos", feature = "benchmark-instrumentation"))]
        crate::pass7_benchmark::mark_pass7_input_admission(self.global.load(Ordering::Acquire));
        self.waker
            .wake()
            .map_err(RuntimeError::AcceptedButWakeFailed)
    }

    pub fn accepted_but_unwritten_bytes(&self) -> usize {
        self.per_execution.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicUsize, Arc};

    use super::InputReservation;

    #[test]
    fn reservation_drop_releases_exactly_once() {
        let global = Arc::new(AtomicUsize::new(0));
        let local = Arc::new(AtomicUsize::new(0));
        {
            let mut reservation =
                InputReservation::reserve(Arc::clone(&global), 32, Arc::clone(&local), 16, 12)
                    .unwrap();
            reservation.consume(5);
            assert_eq!(global.load(std::sync::atomic::Ordering::Acquire), 7);
            assert_eq!(local.load(std::sync::atomic::Ordering::Acquire), 7);
        }
        assert_eq!(global.load(std::sync::atomic::Ordering::Acquire), 0);
        assert_eq!(local.load(std::sync::atomic::Ordering::Acquire), 0);
    }

    #[test]
    fn per_execution_failure_rolls_back_global_reservation() {
        let global = Arc::new(AtomicUsize::new(0));
        let local = Arc::new(AtomicUsize::new(15));
        assert!(
            InputReservation::reserve(Arc::clone(&global), 32, Arc::clone(&local), 16, 2,).is_err()
        );
        assert_eq!(global.load(std::sync::atomic::Ordering::Acquire), 0);
        assert_eq!(local.load(std::sync::atomic::Ordering::Acquire), 15);
    }
}
