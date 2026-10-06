//! G10: current seyal-core prefix versus a getentropy prefix.
//! The current mixer is a measurement replica of `process_id_prefix` on master
//! `b503154d` (`crates/seyal-core/src/lib.rs`). It does not link the production crate.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::stats::Dist;
use crate::unix;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, serde::Serialize)]
pub struct G10Report {
    pub current_prefix_once: Dist,
    pub current_prefix_cached_read: Dist,
    pub getentropy_prefix: Dist,
    pub samples: u32,
    pub current_example_prefix: String,
    pub entropy_example_prefix: String,
}

pub fn run() -> G10Report {
    let mut once = Vec::new();
    for _ in 0..2_000 {
        let started = Instant::now();
        let value = current_prefix();
        std::hint::black_box(value);
        once.push(started.elapsed().as_nanos() as u64);
    }
    let cached = current_prefix();
    let mut hits = Vec::new();
    for _ in 0..20_000 {
        let started = Instant::now();
        std::hint::black_box(cached);
        hits.push(started.elapsed().as_nanos() as u64);
    }
    let mut entropy = Vec::new();
    let mut example = [0_u8; 8];
    for index in 0..20_000 {
        let started = Instant::now();
        unix::fill_entropy(&mut example);
        std::hint::black_box(example);
        entropy.push(started.elapsed().as_nanos() as u64);
        let _ = index;
    }
    G10Report {
        current_prefix_once: Dist::from_samples(&mut once),
        current_prefix_cached_read: Dist::from_samples(&mut hits),
        getentropy_prefix: Dist::from_samples(&mut entropy),
        samples: 20_000,
        current_example_prefix: format!("{:016x}", current_prefix()),
        entropy_example_prefix: format!("{:016x}", u64::from_le_bytes(example)),
    }
}

fn current_prefix() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let low = nanos as u64;
    let high = (nanos >> 64) as u64;
    let pid = std::process::id() as u64;
    let address = (&NEXT_ID as *const AtomicU64 as usize) as u64;
    let _ = NEXT_ID.fetch_add(0, Ordering::Relaxed);
    mix64(low ^ high.rotate_left(17) ^ pid.rotate_left(31) ^ address)
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
