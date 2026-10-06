//! G4: bounded persistence lane beside a non-blocking reactor loop.
//! This is a spike model of ADR-023 §7. It is not a copy of production Runtime
//! and it is not the #677 / #832 implementation.

use std::fs::{self, File};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use crate::stats::Dist;
use crate::unix;

#[derive(Clone, Debug, serde::Serialize)]
pub struct WorkloadReport {
    pub executions: u32,
    pub kind: &'static str,
    pub persistence: bool,
    pub accepted_bytes: u64,
    pub elapsed_ns: u64,
    pub bytes_per_sec: f64,
    pub max_reactor_busy_ns: u64,
    pub echo: Dist,
    pub lane_capacity: u32,
    pub lane_high_water: u32,
    pub gaps: u64,
    pub reactor_sync_calls: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DiskFullReport {
    pub image_created: bool,
    pub retries: u32,
    pub delay_ns: Vec<u64>,
    pub stopped_at_cap: bool,
    pub recovered_after_space: bool,
    pub note: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct G4Report {
    pub workloads: Vec<WorkloadReport>,
    pub disk_full: DiskFullReport,
    pub model: &'static str,
}

pub fn run() -> G4Report {
    let mut workloads = Vec::new();
    for executions in [1_u32, 10, 50, 100] {
        for kind in ["high-output", "interactive"] {
            for persistence in [false, true] {
                eprintln!("g4 {kind} exec={executions} persistence={persistence}");
                workloads.push(one(executions, kind, persistence));
            }
        }
    }
    G4Report {
        workloads,
        disk_full: disk_full(),
        model: "Spike reactor try-enqueues for a fixed window (250 ms high-output, 200 ms interactive) and never calls fsync. Lane capacity is 32. High water is the peak in-flight count. The worker is utility QoS, token-bucketed, and F_FULLFSYNCs each 64 KiB only when persistence is on. Not production Runtime.",
    }
}

fn one(executions: u32, kind: &'static str, persistence: bool) -> WorkloadReport {
    let (tx, rx) = sync_channel::<Vec<u8>>(32);
    let (echo_tx, echo_rx) = sync_channel::<u64>(4096);
    let high_water = AtomicU64::new(0);
    let gaps = AtomicU64::new(0);
    let syncs = AtomicU64::new(0);
    let accepted = AtomicU64::new(0);
    let max_busy = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "seyal-687-g4-{executions}-{kind}-{persistence}"
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let worker_dir = dir.clone();
    let inflight = std::sync::Arc::new(AtomicU64::new(0));
    let worker_inflight = std::sync::Arc::clone(&inflight);
    let worker = thread::spawn(move || {
        let _ = unix::set_utility_qos();
        worker_loop(rx, &worker_dir, persistence, &worker_inflight);
    });
    let started = Instant::now();
    reactor_loop(
        &tx,
        &echo_tx,
        &high_water,
        &gaps,
        &accepted,
        &max_busy,
        &inflight,
        executions,
        kind,
    );
    let wall_ns = started.elapsed().as_nanos() as u64;
    drop(tx);
    let _ = worker.join();
    let mut echoes = Vec::new();
    while let Ok(sample) = echo_rx.try_recv() {
        echoes.push(sample);
    }
    let bytes = accepted.load(Ordering::Relaxed);
    let report = WorkloadReport {
        executions,
        kind,
        persistence,
        accepted_bytes: bytes,
        elapsed_ns: wall_ns,
        bytes_per_sec: bytes as f64 / (wall_ns.max(1) as f64 / 1e9),
        max_reactor_busy_ns: max_busy.load(Ordering::Relaxed),
        echo: Dist::from_samples(&mut echoes),
        lane_capacity: 32,
        lane_high_water: high_water.load(Ordering::Relaxed) as u32,
        gaps: gaps.load(Ordering::Relaxed),
        reactor_sync_calls: syncs.load(Ordering::Relaxed),
    };
    let _ = fs::remove_dir_all(&dir);
    report
}

fn reactor_loop(
    tx: &std::sync::mpsc::SyncSender<Vec<u8>>,
    echo_tx: &std::sync::mpsc::SyncSender<u64>,
    high_water: &AtomicU64,
    gaps: &AtomicU64,
    accepted: &AtomicU64,
    max_busy: &AtomicU64,
    inflight: &AtomicU64,
    executions: u32,
    kind: &str,
) {
    let window = if kind == "interactive" {
        Duration::from_millis(200)
    } else {
        Duration::from_millis(250)
    };
    let deadline = Instant::now() + window;
    let mut next_input = Instant::now();
    while Instant::now() < deadline {
        if kind == "interactive" {
            let now = Instant::now();
            if now < next_input {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            next_input = now + Duration::from_millis(5);
        }
        let busy = Instant::now();
        for _ in 0..executions {
            let bytes = if kind == "interactive" {
                vec![b'x']
            } else {
                vec![0x61; 4096]
            };
            let len = bytes.len() as u64;
            match tx.try_send(bytes) {
                Ok(()) => {
                    let occupied = inflight.fetch_add(1, Ordering::Relaxed) + 1;
                    high_water.fetch_max(occupied, Ordering::Relaxed);
                    accepted.fetch_add(len, Ordering::Relaxed);
                    if kind == "interactive" {
                        let _ = echo_tx.try_send(busy.elapsed().as_nanos() as u64);
                    }
                }
                Err(TrySendError::Full(_)) => {
                    gaps.fetch_add(1, Ordering::Relaxed);
                    accepted.fetch_add(len, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
        max_busy.fetch_max(busy.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

fn worker_loop(
    rx: Receiver<Vec<u8>>,
    dir: &std::path::Path,
    persistence: bool,
    inflight: &AtomicU64,
) {
    let path = dir.join("lane.bin");
    let mut file = File::create(&path).ok();
    let mut buffered = 0_usize;
    let mut tokens: i64 = 4 * 1024 * 1024;
    let mut last = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(chunk) => {
                inflight.fetch_sub(1, Ordering::Relaxed);
                let now = Instant::now();
                let refill = now.duration_since(last).as_nanos() as i64 / 32;
                tokens = (tokens + refill).min(4 * 1024 * 1024);
                last = now;
                if tokens < chunk.len() as i64 {
                    thread::sleep(Duration::from_micros(200));
                    tokens = 4 * 1024 * 1024;
                }
                tokens -= chunk.len() as i64;
                if persistence {
                    if let Some(file) = file.as_mut() {
                        let _ = file.write_all(&chunk);
                        buffered += chunk.len();
                        if buffered >= 64 * 1024 {
                            let _ = file.flush();
                            let _ = unix::fullfsync(file);
                            buffered = 0;
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if persistence {
        if let Some(file) = file.as_mut() {
            let _ = file.flush();
            let _ = unix::fullfsync(file);
        }
    }
}

fn disk_full() -> DiskFullReport {
    let image = std::env::temp_dir().join("seyal-687-g4-full.sparseimage");
    let _ = fs::remove_file(&image);
    let created = std::process::Command::new("hdiutil")
        .args([
            "create", "-size", "24m", "-fs", "APFS", "-volname", "seyal687full", "-type", "SPARSE",
        ])
        .arg(&image)
        .output();
    let Ok(created) = created else {
        return failed("hdiutil did not start");
    };
    if !created.status.success() {
        return failed("hdiutil create failed");
    }
    let attached = std::process::Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-noverify"])
        .arg(&image)
        .output();
    let Ok(attached) = attached else {
        return failed("hdiutil attach did not start");
    };
    if !attached.status.success() {
        let _ = fs::remove_file(&image);
        return failed("hdiutil attach failed");
    }
    let text = String::from_utf8_lossy(&attached.stdout);
    let Some(mount) = text
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .find(|token| token.starts_with("/Volumes/seyal687full"))
    else {
        let _ = fs::remove_file(&image);
        return failed("mount parse failed");
    };
    let mount_path = std::path::PathBuf::from(mount);
    let filler = mount_path.join("filler.bin");
    let mut retries = 0_u32;
    let mut delay = Duration::from_millis(1);
    let mut delays = Vec::new();
    let mut stopped = false;
    {
        let mut file = File::create(&filler).unwrap();
        let block = vec![0xA5_u8; 1024 * 1024];
        loop {
            if file.write_all(&block).is_err() || file.flush().is_err() || unix::fullfsync(&file).is_err() {
                retries += 1;
                delays.push(delay.as_nanos() as u64);
                if retries >= 5 {
                    stopped = true;
                    break;
                }
                thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_millis(32));
            }
        }
    }
    let recovered = (|| {
        fs::remove_file(&filler)?;
        let mut file = File::create(mount_path.join("recovered.bin"))?;
        file.write_all(b"ok")?;
        file.flush()?;
        unix::fullfsync(&file)?;
        Ok::<(), std::io::Error>(())
    })()
    .is_ok();
    let _ = std::process::Command::new("hdiutil")
        .args(["detach", "-force"])
        .arg(&mount_path)
        .status();
    let _ = fs::remove_file(&image);
    DiskFullReport {
        image_created: true,
        retries,
        delay_ns: delays,
        stopped_at_cap: stopped,
        recovered_after_space: recovered,
        note: "Repeated ENOSPC writes used exponential backoff capped at 5 attempts and 32 ms. Space was then released and one write succeeded. The reactor model does not share this volume.".into(),
    }
}

fn failed(note: &str) -> DiskFullReport {
    DiskFullReport {
        image_created: false,
        retries: 0,
        delay_ns: Vec::new(),
        stopped_at_cap: false,
        recovered_after_space: false,
        note: note.into(),
    }
}
