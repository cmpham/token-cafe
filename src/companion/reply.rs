//! Strict, independently testable fragmented protocol reply validation.
use super::Error;
#[derive(Debug)]
pub(super) struct RemoteStatus(pub(super) u8);
impl std::fmt::Display for RemoteStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Companion returned status {}", self.0)
    }
}
impl std::error::Error for RemoteStatus {}

pub(super) fn decode(bytes: &[u8], op: u8, seq: u32, status: u8) -> Result<Option<Vec<u8>>, Error> {
    if bytes.len() > 32 {
        return Err("Oversized companion response".into());
    }
    if bytes.len() < 16 {
        return Ok(None);
    }
    if &bytes[..4] != b"TCF1" {
        return Err::<Option<Vec<u8>>, Error>("Invalid response magic".into());
    }
    let len = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
    if !(1..=16).contains(&len) {
        return Err("Invalid response length".into());
    }
    if bytes.len() < 16 + len {
        return Ok(None);
    }
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&bytes[..12]);
    hasher.update(&bytes[16..]);
    if bytes.len() != 16 + len
        || bytes[4] != op | 0x80
        || bytes[5] != 1
        || u32::from_le_bytes(bytes[8..12].try_into()?) != seq
        || u32::from_le_bytes(bytes[12..16].try_into()?) != hasher.finalize()
    {
        return Err("Invalid response status/sequence/CRC".into());
    }
    if bytes[16] != status {
        return Err(Box::new(RemoteStatus(bytes[16])));
    }
    Ok(Some(bytes[17..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::packet;
    #[test]
    fn fragments_must_validate_before_status_is_trusted() {
        let response = packet(12 | 0x80, 7, &[0, 3, 1]);
        for n in 0..response.len() {
            assert!(decode(&response[..n], 12, 7, 0).unwrap().is_none());
        }
        assert_eq!(decode(&response, 12, 7, 0).unwrap(), Some(vec![3, 1]));
        assert!(decode(&response, 12, 8, 0).is_err());
        assert!(decode(&response, 11, 7, 0).is_err());
        let unsupported = packet(12 | 0x80, 7, &[3]);
        assert_eq!(decode(&unsupported, 12, 7, 0).unwrap_err().downcast_ref::<RemoteStatus>().unwrap().0, 3);
        let mut corrupt = unsupported.clone();
        corrupt[12] ^= 1;
        assert!(decode(&corrupt, 12, 7, 0).unwrap_err().downcast_ref::<RemoteStatus>().is_none());
        let mut extra = response.clone();
        extra.push(0);
        assert!(decode(&extra, 12, 7, 0).is_err());
        assert!(decode(&[0; 33], 12, 7, 0).is_err());
    }
}
