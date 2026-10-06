//! Append-only checksummed pack of 16 KiB stand-in ADR-010 sealed segments.
//!
//! Each payload is `SEGMENT_LEN` bytes. The production HistoryStore encoder is
//! not linked; the payload is a calibration stand-in at the M002 16 KiB target.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::unix;

pub const SEGMENT_LEN: usize = 16 * 1024;
pub const PACK_MAGIC: &[u8; 4] = b"SEYP";
pub const FRAME_MAGIC: &[u8; 4] = b"SEG1";
pub const PACK_HEADER_LEN: usize = 16;
pub const FRAME_HEADER_LEN: usize = 24;
const MAX_STORED: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    EachRecord,
    EachPack,
    /// Userspace flush only. Used to reach the after-append/before-sync crash boundary.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Codec {
    None = 0,
    Lz4 = 1,
    Zstd1 = 2,
    Zstd3 = 3,
}

impl Codec {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Lz4),
            2 => Some(Self::Zstd1),
            3 => Some(Self::Zstd3),
            _ => None,
        }
    }

    pub fn all() -> &'static [Codec] {
        &[Self::None, Self::Lz4, Self::Zstd1, Self::Zstd3]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lz4 => "lz4",
            Self::Zstd1 => "zstd-1",
            Self::Zstd3 => "zstd-3",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub seq: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum PackError {
    Truncated { at: usize },
    BadMagic,
    BadCodec,
    TooLarge,
    BadCrc { seq: u64 },
    Io(std::io::Error),
    Codec(String),
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated { at } => write!(f, "truncated pack at {at}"),
            Self::BadMagic => write!(f, "bad pack magic"),
            Self::BadCodec => write!(f, "bad codec"),
            Self::TooLarge => write!(f, "frame exceeds bound"),
            Self::BadCrc { seq } => write!(f, "checksum mismatch for segment {seq}"),
            Self::Io(err) => write!(f, "pack io: {err}"),
            Self::Codec(err) => write!(f, "codec: {err}"),
        }
    }
}

impl std::error::Error for PackError {}

impl From<std::io::Error> for PackError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn compress(codec: Codec, raw: &[u8]) -> Result<Vec<u8>, PackError> {
    match codec {
        Codec::None => Ok(raw.to_vec()),
        Codec::Lz4 => Ok(lz4_flex::compress_prepend_size(raw)),
        Codec::Zstd1 => zstd::bulk::compress(raw, 1).map_err(|err| PackError::Codec(err.to_string())),
        Codec::Zstd3 => zstd::bulk::compress(raw, 3).map_err(|err| PackError::Codec(err.to_string())),
    }
}

pub fn decompress(codec: Codec, stored: &[u8], raw_len: usize) -> Result<Vec<u8>, PackError> {
    let raw = match codec {
        Codec::None => stored.to_vec(),
        Codec::Lz4 => lz4_flex::decompress_size_prepended(stored)
            .map_err(|err| PackError::Codec(err.to_string()))?,
        Codec::Zstd1 | Codec::Zstd3 => zstd::bulk::decompress(stored, raw_len)
            .map_err(|err| PackError::Codec(err.to_string()))?,
    };
    if raw.len() != raw_len {
        return Err(PackError::Codec("decoded length mismatch".into()));
    }
    Ok(raw)
}

pub fn encode_frame(codec: Codec, seq: u64, raw: &[u8]) -> Result<Vec<u8>, PackError> {
    let stored = compress(codec, raw)?;
    if stored.len() > MAX_STORED {
        return Err(PackError::TooLarge);
    }
    let crc = crc32fast::hash(raw);
    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + stored.len());
    frame.extend_from_slice(FRAME_MAGIC);
    frame.extend_from_slice(&seq.to_le_bytes());
    frame.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(stored.len() as u32).to_le_bytes());
    frame.extend_from_slice(&crc.to_le_bytes());
    frame.extend_from_slice(&stored);
    Ok(frame)
}

pub fn parse_pack(bytes: &[u8]) -> Result<ParsedPack, PackError> {
    if bytes.len() < PACK_HEADER_LEN {
        return Err(PackError::Truncated { at: 0 });
    }
    if &bytes[0..4] != PACK_MAGIC {
        return Err(PackError::BadMagic);
    }
    let codec = Codec::from_u8(bytes[6]).ok_or(PackError::BadCodec)?;
    let mut offset = PACK_HEADER_LEN;
    let mut segments = Vec::new();
    while offset < bytes.len() {
        if bytes.len() - offset < FRAME_HEADER_LEN {
            return Err(PackError::Truncated { at: offset });
        }
        if &bytes[offset..offset + 4] != FRAME_MAGIC {
            return Err(PackError::BadMagic);
        }
        let seq = u64::from_le_bytes(bytes[offset + 4..offset + 12].try_into().unwrap());
        let raw_len = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap()) as usize;
        let stored_len =
            u32::from_le_bytes(bytes[offset + 16..offset + 20].try_into().unwrap()) as usize;
        let crc = u32::from_le_bytes(bytes[offset + 20..offset + 24].try_into().unwrap());
        if raw_len > SEGMENT_LEN || stored_len > MAX_STORED {
            return Err(PackError::TooLarge);
        }
        let start = offset + FRAME_HEADER_LEN;
        let end = start.saturating_add(stored_len);
        if end > bytes.len() {
            return Err(PackError::Truncated { at: offset });
        }
        let raw = decompress(codec, &bytes[start..end], raw_len)?;
        if crc32fast::hash(&raw) != crc {
            return Err(PackError::BadCrc { seq });
        }
        segments.push(Segment { seq, payload: raw });
        offset = end;
    }
    Ok(ParsedPack { codec, segments })
}

#[derive(Debug)]
pub struct ParsedPack {
    pub codec: Codec,
    pub segments: Vec<Segment>,
}

fn header(codec: Codec) -> [u8; PACK_HEADER_LEN] {
    let mut header = [0_u8; PACK_HEADER_LEN];
    header[0..4].copy_from_slice(PACK_MAGIC);
    header[4..6].copy_from_slice(&1_u16.to_le_bytes());
    header[6] = codec as u8;
    let crc = crc32fast::hash(&header[0..8]);
    header[8..12].copy_from_slice(&crc.to_le_bytes());
    header
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SegmentLoc {
    pub pack_index: u32,
    pub seq: u64,
    pub file_offset: u64,
    pub stored_len: u32,
    pub raw_len: u32,
}

#[derive(Debug)]
pub struct PackWrite {
    pub files: Vec<PathBuf>,
    pub locs: Vec<SegmentLoc>,
    pub logical_bytes: u64,
    pub stored_bytes: u64,
    pub write_syscall_bytes: u64,
    pub append_ns: Vec<u64>,
    pub file_checksums: Vec<u32>,
}

pub struct PackWriter {
    dir: PathBuf,
    codec: Codec,
    roll_bytes: u64,
    sync: SyncMode,
    file: Option<File>,
    path: Option<PathBuf>,
    index: u32,
    current_len: u64,
    pub write: PackWrite,
}

impl PackWriter {
    pub fn create(dir: &Path, codec: Codec, roll_bytes: u64, sync: SyncMode) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            codec,
            roll_bytes,
            sync,
            file: None,
            path: None,
            index: 0,
            current_len: 0,
            write: PackWrite {
                files: Vec::new(),
                locs: Vec::new(),
                logical_bytes: 0,
                stored_bytes: 0,
                write_syscall_bytes: 0,
                append_ns: Vec::new(),
                file_checksums: Vec::new(),
            },
        })
    }

    fn open_next(&mut self) -> Result<(), PackError> {
        let path = self.dir.join(format!("pack-{index:06}.spk", index = self.index));
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)?;
        let header = header(self.codec);
        file.write_all(&header)?;
        self.write.write_syscall_bytes += header.len() as u64;
        self.current_len = header.len() as u64;
        self.write.stored_bytes += header.len() as u64;
        self.path = Some(path.clone());
        self.write.files.push(path);
        self.file = Some(file);
        self.index += 1;
        Ok(())
    }
}

impl PackWriter {
    fn ensure_room(&mut self, frame_len: u64) -> Result<(), PackError> {
        let needs_roll = match &self.file {
            None => true,
            Some(_) => self.current_len.saturating_add(frame_len) > self.roll_bytes,
        };
        if needs_roll {
            self.close_current()?;
            self.open_next()?;
        }
        Ok(())
    }

    fn close_current(&mut self) -> Result<(), PackError> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        file.flush()?;
        if self.sync != SyncMode::None {
            unix::fullfsync(file)?;
        }
        if let Some(path) = &self.path {
            let bytes = fs::read(path)?;
            self.write.file_checksums.push(crc32fast::hash(&bytes));
        }
        self.file = None;
        self.path = None;
        self.current_len = 0;
        Ok(())
    }
}

pub fn write_segments(
    dir: &Path,
    codec: Codec,
    roll_bytes: u64,
    sync: SyncMode,
    segments: &[Vec<u8>],
) -> Result<PackWrite, PackError> {
    let mut writer = PackWriter::create(dir, codec, roll_bytes, sync)?;
    for (seq, raw) in segments.iter().enumerate() {
        let frame = encode_frame(codec, seq as u64, raw)?;
        writer.ensure_room(frame.len() as u64)?;
        let started = Instant::now();
        let file = writer.file.as_mut().expect("open pack");
        let offset = writer.current_len;
        file.write_all(&frame)?;
        writer.write.write_syscall_bytes += frame.len() as u64;
        writer.current_len += frame.len() as u64;
        writer.write.stored_bytes += frame.len() as u64;
        writer.write.logical_bytes += raw.len() as u64;
        if writer.sync == SyncMode::EachRecord {
            file.flush()?;
            unix::fullfsync(file)?;
        }
        writer.write.locs.push(SegmentLoc {
            pack_index: writer.index - 1,
            seq: seq as u64,
            file_offset: offset,
            stored_len: (frame.len() - FRAME_HEADER_LEN) as u32,
            raw_len: raw.len() as u32,
        });
        writer.write.append_ns.push(started.elapsed().as_nanos() as u64);
    }
    writer.close_current()?;
    Ok(writer.write)
}

pub fn read_segment(path: &Path, codec: Codec, loc: &SegmentLoc) -> Result<Vec<u8>, PackError> {
    let mut file = File::open(path)?;
    let _ = unix::set_no_cache(&file);
    file.seek(SeekFrom::Start(loc.file_offset))?;
    let mut header = [0_u8; FRAME_HEADER_LEN];
    file.read_exact(&mut header)?;
    if &header[0..4] != FRAME_MAGIC {
        return Err(PackError::BadMagic);
    }
    let stored_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let raw_len = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    let crc = u32::from_le_bytes(header[20..24].try_into().unwrap());
    let mut stored = vec![0_u8; stored_len];
    file.read_exact(&mut stored)?;
    let raw = decompress(codec, &stored, raw_len)?;
    if crc32fast::hash(&raw) != crc {
        return Err(PackError::BadCrc { seq: loc.seq });
    }
    Ok(raw)
}

/// Build mixed segments: M002 fixture bytes cycled into a header, synthetic
/// high-output lines, and every 10th segment mostly incompressible.
pub fn build_corpus(segment_count: usize, fixture_bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::with_capacity(segment_count);
    for seq in 0..segment_count {
        let mut raw = vec![0_u8; SEGMENT_LEN];
        raw[0..8].copy_from_slice(&(seq as u64).to_le_bytes());
        raw[8..16].copy_from_slice(&((seq as u64).saturating_add(32)).to_le_bytes());
        if !fixture_bytes.is_empty() {
            let start = (seq * 64) % fixture_bytes.len();
            let take = 64.min(fixture_bytes.len());
            for (index, byte) in fixture_bytes.iter().cycle().skip(start).take(take).enumerate() {
                raw[16 + index] = *byte;
            }
        }
        if seq % 10 == 0 {
            let mut state = 0x9e37_79b9_u64.wrapping_add(seq as u64);
            for chunk in raw[80..].chunks_mut(8) {
                state = state
                    .wrapping_mul(0xbf58_476d_1ce4_e5b9)
                    .wrapping_add(0x94d0_49bb_1331_11eb);
                let bytes = state.to_le_bytes();
                chunk.copy_from_slice(&bytes[..chunk.len()]);
            }
        } else {
            let mut offset = 80;
            let mut line = 0_u64;
            while offset + 80 <= SEGMENT_LEN {
                let text = format!(
                    "2026-10-05T00:00:00Z stream=stdout seq={seq:08} line={line:06} token=alpha\n"
                );
                let bytes = text.as_bytes();
                let copy = bytes.len().min(SEGMENT_LEN - offset);
                raw[offset..offset + copy].copy_from_slice(&bytes[..copy]);
                offset += copy;
                line += 1;
            }
        }
        out.push(raw);
    }
    out
}

pub fn load_m002_fixtures(dir: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("m002-") && (name.ends_with(".input") || name.ends_with(".expected.txt")) {
            if let Ok(bytes) = fs::read(&path) {
                out.extend_from_slice(&bytes);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_torn_tail() {
        let raw = vec![7_u8; SEGMENT_LEN];
        let frame = encode_frame(Codec::Zstd1, 4, &raw).unwrap();
        let mut bytes = header(Codec::Zstd1).to_vec();
        bytes.extend_from_slice(&frame);
        let parsed = parse_pack(&bytes).unwrap();
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].payload, raw);
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        assert!(matches!(
            parse_pack(&bytes),
            Err(PackError::Truncated { .. })
        ));
    }
}
