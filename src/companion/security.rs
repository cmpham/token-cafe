//! Per-connection, directional authenticated fragments. Counters reject replay.
use super::Error;
use hmac::{Hmac, Mac};
use sha2::Sha256;
pub(super) struct Channel {
    key: [u8; 32],
    challenge: Vec<u8>,
    tx: u32,
    rx: u32,
}
impl Channel {
    pub fn new(key: [u8; 32], challenge: Vec<u8>) -> Self {
        Self { key, challenge, tx: 0, rx: 0 }
    }
    fn mac(&self, role: &[u8], bytes: &[u8]) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).unwrap();
        mac.update(role);
        mac.update(&self.challenge);
        mac.update(bytes);
        mac
    }
    pub fn seal(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        self.tx = self.tx.checked_add(1).ok_or("Transport counter exhausted")?;
        let mut out = self.tx.to_le_bytes().to_vec();
        out.extend(data);
        let tag = self.mac(b"host-data", &out).finalize().into_bytes();
        out.extend(&tag[..16]);
        Ok(out)
    }
    pub fn open(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        if data.len() <= 20 {
            return Err("Invalid authenticated fragment".into());
        }
        let end = data.len() - 16;
        self.mac(b"board-data", &data[..end])
            .verify_truncated_left(&data[end..])
            .map_err(|_| "Invalid fragment authentication")?;
        let counter = u32::from_le_bytes(data[..4].try_into()?);
        if self.rx.checked_add(1) != Some(counter) {
            return Err("Replayed or missing fragment".into());
        }
        self.rx = counter;
        Ok(data[4..end].to_vec())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_tampering_replay_wrong_role_and_connection() {
        let mut c = Channel::new([9; 32], vec![7; 32]);
        let mut message = 1u32.to_le_bytes().to_vec();
        message.extend(b"tap");
        let tag = c.mac(b"board-data", &message).finalize().into_bytes();
        message.extend(&tag[..16]);
        let mut corrupt = message.clone();
        corrupt[4] ^= 1;
        assert!(c.open(&corrupt).is_err());
        assert_eq!(c.open(&message).unwrap(), b"tap");
        assert!(c.open(&message).is_err());
        assert!(Channel::new([9; 32], vec![8; 32]).open(&message).is_err());
        let host = c.seal(b"tap").unwrap();
        assert!(Channel::new([9; 32], vec![7; 32]).open(&host).is_err());
    }
}
