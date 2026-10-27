use crate::{
    crypto::{Error, Result},
    wire::{Envelope, envelope::Payload},
};
use alloc::vec::Vec;
use prost::Message;
unsafe extern "C" {
    fn tc_wire_validate(bytes: *const u8, size: usize) -> i32;
    fn tc_wire_roundtrip(bytes: *const u8, size: usize, out: *mut u8, length: *mut usize) -> i32;
}
pub fn control(payload: &Payload) -> bool {
    !matches!(
        payload,
        Payload::SnapshotBegin(_)
            | Payload::SnapshotPart(_)
            | Payload::SnapshotCommit(_)
            | Payload::Delta(_)
            | Payload::BlobChunk(_)
    )
}
pub fn decode(bytes: &[u8], lane: bool) -> Result<Envelope> {
    if bytes.is_empty() || bytes.len() > if lane { 1024 } else { 4096 } {
        return Err(Error::Limit);
    }
    // Nanopb has statically bounded strings and arrays. It rejects hostile counts
    // before prost allocates any vectors. The same C code runs on host and board.
    if unsafe { tc_wire_validate(bytes.as_ptr(), bytes.len()) } == 0 {
        return Err(Error::Invalid);
    }
    let message = Envelope::decode(bytes).map_err(|_| Error::Invalid)?;
    if message.major != 2 || message.connection_epoch.len() != 16 {
        return Err(Error::Invalid);
    }
    let payload = message.payload.as_ref().ok_or(Error::Invalid)?;
    if control(payload) != lane {
        return Err(Error::Invalid);
    }
    if !matches!(payload, Payload::Hello(_)) && message.authority_epoch.len() != 16 {
        return Err(Error::Invalid);
    }
    Ok(message)
}
pub fn encode(message: &Envelope) -> Result<Vec<u8>> {
    let payload = message.payload.as_ref().ok_or(Error::Invalid)?;
    let n = message.encoded_len();
    if n > if control(payload) { 1024 } else { 4096 } {
        return Err(Error::Limit);
    }
    let bytes = message.encode_to_vec();
    decode(&bytes, control(payload))?;
    Ok(bytes)
}
pub fn c_roundtrip(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = alloc::vec![0u8;4096];
    let mut len = out.len();
    if unsafe { tc_wire_roundtrip(bytes.as_ptr(), bytes.len(), out.as_mut_ptr(), &mut len) } == 0 {
        return Err(Error::Invalid);
    }
    out.truncate(len);
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::*;
    #[test]
    fn c_rust_roundtrip_and_bounds() {
        let m = Envelope {
            major: 2,
            connection_epoch: alloc::vec![1;16],
            authority_epoch: alloc::vec![2;16],
            payload: Some(Payload::Action(Action {
                id: 3,
                kind: 1,
                session_id: "claude:qa".into(),
                ..Default::default()
            })),
        };
        let bytes = encode(&m).unwrap();
        assert_eq!(decode(&c_roundtrip(&bytes).unwrap(), true).unwrap(), m);
        assert!(decode(&bytes, false).is_err());
        let mut bad = m.clone();
        if let Some(Payload::Action(a)) = bad.payload.as_mut() {
            a.session_id = "x".repeat(129);
        }
        assert!(encode(&bad).is_err());
    }
    #[test]
    fn oversized_repeated_fields_rejected_before_prost() {
        let m = Envelope {
            major: 2,
            connection_epoch: alloc::vec![1;16],
            authority_epoch: alloc::vec![2;16],
            payload: Some(Payload::SnapshotPart(SnapshotPart {
                revision: 1,
                index: 0,
                sessions: alloc::vec![Session::default();3],
            })),
        };
        assert!(decode(&m.encode_to_vec(), false).is_err());
    }
}
