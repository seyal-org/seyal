//! Binary helpers for MemoryStore rows.

use seyal_agent_core::{EvidenceRef, MemoryId};

pub(crate) fn encode_evidence(refs: &[EvidenceRef]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(refs.len() as u32).to_le_bytes());
    for r in refs {
        out.extend_from_slice(&r.kind.to_le_bytes());
        out.extend_from_slice(&(r.bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&r.bytes);
    }
    out
}

pub(crate) fn decode_evidence(bytes: &[u8]) -> Result<Vec<EvidenceRef>, ()> {
    if bytes.len() < 4 {
        return Err(());
    }
    let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let mut offset = 4;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.len() < offset + 6 {
            return Err(());
        }
        let kind = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
        offset += 2;
        let len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if bytes.len() < offset + len {
            return Err(());
        }
        out.push(EvidenceRef {
            kind,
            bytes: bytes[offset..offset + len].to_vec(),
        });
        offset += len;
    }
    Ok(out)
}

pub(crate) fn encode_id_list(ids: &[MemoryId]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
    for id in ids {
        out.extend_from_slice(&id.to_bytes());
    }
    out
}

pub(crate) fn decode_id_list(bytes: &[u8]) -> Result<Vec<MemoryId>, ()> {
    if bytes.len() < 4 {
        return Err(());
    }
    let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let mut offset = 4;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.len() < offset + 16 {
            return Err(());
        }
        let mut id = [0u8; 16];
        id.copy_from_slice(&bytes[offset..offset + 16]);
        out.push(MemoryId::from_bytes(id));
        offset += 16;
    }
    Ok(out)
}

pub(crate) fn encode_fingerprints(fps: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(fps.len() as u32).to_le_bytes());
    for fp in fps {
        out.extend_from_slice(&(fp.len() as u32).to_le_bytes());
        out.extend_from_slice(fp);
    }
    out
}

pub(crate) fn decode_fingerprints(bytes: &[u8]) -> Result<Vec<Vec<u8>>, ()> {
    if bytes.len() < 4 {
        return Err(());
    }
    let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let mut offset = 4;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.len() < offset + 4 {
            return Err(());
        }
        let len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if bytes.len() < offset + len {
            return Err(());
        }
        out.push(bytes[offset..offset + len].to_vec());
        offset += len;
    }
    Ok(out)
}

pub(crate) fn opaque_token(
    key: &[u8; 32],
    scope_kind: u8,
    scope_id: &[u8; 16],
    material: &[u8],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(key);
    hasher.update(&[scope_kind]);
    hasher.update(scope_id);
    hasher.update(material);
    *hasher.finalize().as_bytes()
}
