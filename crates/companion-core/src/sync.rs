//! Shared publisher: revision deltas where bounded, atomic snapshots otherwise.
use crate::{
    codec,
    crypto::{Error, Result},
    model::{Model, validate},
    wire::{envelope::Payload, *},
};
use alloc::vec::Vec;
use sha2::{Digest, Sha256};
pub fn envelope(connection: &[u8], authority: &[u8], payload: Payload) -> Envelope {
    Envelope {
        major: 2,
        connection_epoch: connection.to_vec(),
        authority_epoch: authority.to_vec(),
        payload: Some(payload),
    }
}
pub fn snapshot(m: &Model, c: &[u8], a: &[u8]) -> Result<Vec<Envelope>> {
    validate(m)?;
    let mut out = Vec::new();
    let mut hash = Sha256::new();
    let b = envelope(
        c,
        a,
        Payload::SnapshotBegin(SnapshotBegin {
            revision: m.revision,
            session_count: m.sessions.len() as u32,
            view: Some(m.view.clone()),
            usage: Some(m.usage.clone()),
            overflow_count: m.overflow_count,
        }),
    );
    let bytes = codec::encode(&b)?;
    hash.update((bytes.len() as u32).to_le_bytes());
    hash.update(&bytes);
    out.push(b);
    let mut it = m.sessions.values().peekable();
    let mut index = 0;
    while let Some(s) = it.next() {
        let mut part = SnapshotPart { revision: m.revision, index, sessions: alloc::vec![s.clone()] };
        let mut e = envelope(c, a, Payload::SnapshotPart(part.clone()));
        codec::encode(&e)?;
        if let Some(next) = it.peek() {
            part.sessions.push((*next).clone());
            let candidate = envelope(c, a, Payload::SnapshotPart(part));
            if codec::encode(&candidate).is_ok() {
                e = candidate;
                it.next();
            }
        }
        let bytes = codec::encode(&e)?;
        hash.update((bytes.len() as u32).to_le_bytes());
        hash.update(&bytes);
        out.push(e);
        index += 1;
    }
    out.push(envelope(
        c,
        a,
        Payload::SnapshotCommit(SnapshotCommit {
            revision: m.revision,
            parts_count: index,
            sha256: hash.finalize().to_vec(),
        }),
    ));
    Ok(out)
}
pub fn update(old: &Model, new: &Model, c: &[u8], a: &[u8]) -> Result<Vec<Envelope>> {
    validate(new)?;
    if new.revision <= old.revision {
        return Err(Error::Sequence);
    }
    let d = Delta {
        base_revision: old.revision,
        revision: new.revision,
        upsert_sessions: new
            .sessions
            .iter()
            .filter(|(id, s)| old.sessions.get(*id) != Some(*s))
            .map(|(_, s)| s.clone())
            .collect(),
        remove_session_ids: old.sessions.keys().filter(|id| !new.sessions.contains_key(*id)).cloned().collect(),
        view: (old.view != new.view).then(|| new.view.clone()),
        usage: (old.usage != new.usage).then(|| new.usage.clone()),
        overflow_count: (old.overflow_count != new.overflow_count).then_some(new.overflow_count),
    };
    if d.upsert_sessions.len() <= 2 && d.remove_session_ids.len() <= 8 {
        let e = envelope(c, a, Payload::Delta(d));
        if codec::encode(&e).is_ok() {
            return Ok(alloc::vec![e]);
        }
    }
    snapshot(new, c, a)
}
pub fn blob(bytes: &[u8]) -> Result<BlobRef> {
    if bytes.len() > 32768 || core::str::from_utf8(bytes).is_err() {
        return Err(Error::Limit);
    }
    Ok(BlobRef { sha256: crate::crypto::hash(bytes).to_vec(), byte_length: bytes.len() as u32 })
}
pub fn chunks(bytes: &[u8]) -> Result<Vec<BlobChunk>> {
    let b = blob(bytes)?;
    let mut result = Vec::new();
    if bytes.is_empty() {
        result.push(BlobChunk { sha256: b.sha256, total_length: 0, offset: 0, data: Vec::new(), r#final: true });
    } else {
        for (n, c) in bytes.chunks(512).enumerate() {
            result.push(BlobChunk {
                sha256: b.sha256.clone(),
                total_length: b.byte_length,
                offset: (n * 512) as u32,
                data: c.to_vec(),
                r#final: (n * 512 + c.len()) == bytes.len(),
            });
        }
    }
    Ok(result)
}
