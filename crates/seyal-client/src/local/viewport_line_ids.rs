//! SPEC-004 §8.1 `ViewportLineIds` pairing for one local attachment.
//!
//! The vector is accepted only when its generation equals the committed
//! display generation and its length equals the viewport row count. A newer
//! generation is discarded. A conflicting vector for the same generation is
//! a protocol error.

use seyal_runtime::local_ipc::framing::ViewportLineIds;

use super::{ClientError, LocalDisplayClient};

impl LocalDisplayClient {
    /// Latest Runtime-published primary viewport LineIds for Flow live-tail
    /// mapping. Empty until a `ViewportLineIds` frame is accepted for this
    /// attachment, and cleared on disconnect/resync.
    pub fn viewport_line_ids(&self) -> &[u64] {
        &self.viewport_line_ids
    }

    pub fn viewport_line_ids_generation(&self) -> u64 {
        self.viewport_line_ids_generation
    }

    pub(crate) fn clear_viewport_line_ids(&mut self) {
        self.viewport_line_ids.clear();
        self.viewport_line_ids_generation = 0;
    }

    /// Test-only: pair LineIds with a committed display generation for FFI
    /// projection checks (production path uses Runtime ViewportLineIds frames).
    #[cfg(test)]
    pub(crate) fn set_paired_viewport_line_ids_for_test(
        &mut self,
        generation: u64,
        line_ids: Vec<u64>,
    ) {
        self.cache.generation = generation;
        self.cache.rows = line_ids.len() as u16;
        self.viewport_line_ids_generation = generation;
        self.viewport_line_ids = line_ids;
    }

    /// Apply one decoded type-35 payload. `Ok(true)` means attachment metadata
    /// changed. Newer-than-display generations are discarded without clearing
    /// a vector still paired to the committed display.
    pub(crate) fn apply_viewport_line_ids(&mut self, payload: &[u8]) -> Result<bool, ClientError> {
        let message = ViewportLineIds::decode(payload).map_err(|_| ClientError::Protocol)?;
        if message.generation > self.cache.generation {
            return Ok(false);
        }
        if message.generation < self.viewport_line_ids_generation {
            return Ok(false);
        }
        if message.generation == self.viewport_line_ids_generation
            && message.line_ids != self.viewport_line_ids
        {
            return Err(ClientError::Protocol);
        }
        if message.generation != self.cache.generation
            || message.line_ids.len() != usize::from(self.cache.rows)
        {
            if self.viewport_line_ids.is_empty() {
                return Ok(false);
            }
            self.clear_viewport_line_ids();
            return Ok(true);
        }
        let changed = self.viewport_line_ids_generation != message.generation
            || self.viewport_line_ids != message.line_ids;
        self.viewport_line_ids = message.line_ids;
        self.viewport_line_ids_generation = message.generation;
        Ok(changed)
    }

    /// Drop a vector whose generation no longer matches the committed display.
    pub(crate) fn drop_unpaired_viewport_line_ids(&mut self) -> bool {
        if self.viewport_line_ids_generation != 0
            && self.viewport_line_ids_generation != self.cache.generation
        {
            self.clear_viewport_line_ids();
            true
        } else {
            false
        }
    }
}
