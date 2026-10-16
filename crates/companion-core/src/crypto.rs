//! Exact v2 handshake, request identity and encrypted two-lane fragments.
use alloc::{string::String, vec::Vec};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Authentication,
    Counter,
    Limit,
    Sequence,
}
pub type Result<T> = core::result::Result<T, Error>;
pub fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn lp(out: &mut Vec<u8>, text: &str, max: usize) -> Result<()> {
    if text.len() > max {
        return Err(Error::Limit);
    }
    out.extend_from_slice(&(text.len() as u16).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    Ok(())
}
pub fn transcript(device: &str, host: &[u8; 16], board: &[u8; 16]) -> Result<Vec<u8>> {
    if device.is_empty() || !device.is_ascii() {
        return Err(Error::Invalid);
    }
    let mut out = b"tc-auth-v2".to_vec();
    lp(&mut out, device, 64)?;
    out.extend_from_slice(host);
    out.extend_from_slice(board);
    Ok(out)
}
pub fn auth_tag(key: &[u8; 32], role: &[u8], transcript: &[u8]) -> [u8; 16] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).unwrap();
    mac.update(role);
    mac.update(transcript);
    mac.finalize().into_bytes()[..16].try_into().unwrap()
}
pub fn verify_auth(key: &[u8; 32], role: &[u8], t: &[u8], tag: &[u8]) -> Result<()> {
    if tag.len() != 16 {
        return Err(Error::Authentication);
    }
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).unwrap();
    mac.update(role);
    mac.update(t);
    mac.verify_truncated_left(tag).map_err(|_| Error::Authentication)
}
pub struct Keys {
    pub host: [u8; 32],
    pub board: [u8; 32],
    pub host_nonce: [u8; 4],
    pub board_nonce: [u8; 4],
    pub epoch: [u8; 16],
}
impl Keys {
    pub fn derive(key: &[u8; 32], transcript: &[u8]) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(&hash(transcript)), key);
        let mut k = Self { host: [0; 32], board: [0; 32], host_nonce: [0; 4], board_nonce: [0; 4], epoch: [0; 16] };
        hk.expand(b"tc-v2/host-key", &mut k.host).unwrap();
        hk.expand(b"tc-v2/board-key", &mut k.board).unwrap();
        hk.expand(b"tc-v2/host-nonce", &mut k.host_nonce).unwrap();
        hk.expand(b"tc-v2/board-nonce", &mut k.board_nonce).unwrap();
        hk.expand(b"tc-v2/epoch", &mut k.epoch).unwrap();
        k
    }
}
// Arguments follow the normative digest preimage order.
#[allow(clippy::too_many_arguments)]
pub fn request_digest(
    request: &str,
    session: &str,
    provider: u8,
    title: &str,
    tool: &str,
    body: &[u8; 32],
    body_len: u32,
    decisions: &[u8],
) -> Result<[u8; 32]> {
    if request.is_empty()
        || session.is_empty()
        || !(1..=2).contains(&provider)
        || body_len > 32768
        || decisions.len() > 4
        || decisions.iter().any(|x| !(1..=6).contains(x))
        || decisions.windows(2).any(|x| x[0] >= x[1])
    {
        return Err(Error::Invalid);
    }
    let mut out = b"tc-req-v2".to_vec();
    lp(&mut out, request, 128)?;
    lp(&mut out, session, 128)?;
    out.push(provider);
    lp(&mut out, title, 192)?;
    lp(&mut out, tool, 192)?;
    out.extend_from_slice(body);
    out.extend_from_slice(&body_len.to_le_bytes());
    out.push(decisions.len() as u8);
    out.extend_from_slice(decisions);
    Ok(hash(&out))
}
/// Separate instances for each direction, sharing only that direction's counter.
pub struct Cipher {
    cipher: ChaCha20Poly1305,
    prefix: [u8; 4],
    direction: u8,
    counter: u64,
    failed: bool,
}
impl Cipher {
    pub fn new(key: &[u8; 32], prefix: [u8; 4], direction: u8) -> Self {
        Self { cipher: ChaCha20Poly1305::new(key.into()), prefix, direction, counter: 0, failed: false }
    }
    fn parts(&self, counter: u64) -> ([u8; 12], Vec<u8>) {
        let mut n = [0; 12];
        n[..4].copy_from_slice(&self.prefix);
        n[4..].copy_from_slice(&counter.to_le_bytes());
        let mut ad = b"tc-v2".to_vec();
        ad.push(self.direction);
        ad.extend_from_slice(&counter.to_le_bytes());
        (n, ad)
    }
    pub fn seal(&mut self, id: u32, total: u32, offset: u32, data: &[u8]) -> Result<Vec<u8>> {
        if self.failed {
            return Err(Error::Authentication);
        }
        let counter = self.counter.checked_add(1).ok_or(Error::Counter)?;
        let mut p = Vec::with_capacity(12 + data.len());
        p.extend_from_slice(&id.to_le_bytes());
        p.extend_from_slice(&total.to_le_bytes());
        p.extend_from_slice(&offset.to_le_bytes());
        p.extend_from_slice(data);
        let (n, ad) = self.parts(counter);
        let enc = self
            .cipher
            .encrypt(Nonce::from_slice(&n), Payload { msg: &p, aad: &ad })
            .map_err(|_| Error::Authentication)?;
        let mut out = counter.to_le_bytes().to_vec();
        out.extend(enc);
        self.counter = counter;
        Ok(out)
    }
    pub fn open(&mut self, wire: &[u8]) -> Result<Vec<u8>> {
        if self.failed {
            return Err(Error::Authentication);
        }
        let result = (|| {
            if wire.len() < 36 || wire.len() > 512 {
                return Err(Error::Limit);
            }
            let c = u64::from_le_bytes(wire[..8].try_into().unwrap());
            if self.counter.checked_add(1) != Some(c) {
                return Err(Error::Counter);
            }
            let (n, ad) = self.parts(c);
            let p = self
                .cipher
                .decrypt(Nonce::from_slice(&n), Payload { msg: &wire[8..], aad: &ad })
                .map_err(|_| Error::Authentication)?;
            self.counter = c;
            Ok(p)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
#[derive(Default)]
struct Lane {
    id: u32,
    total: usize,
    bytes: Vec<u8>,
    last_ms: u64,
    partial: bool,
}
#[derive(Default)]
pub struct Reassembly {
    lanes: [Lane; 2],
    failed: bool,
}
impl Reassembly {
    pub fn receive(&mut self, plain: &[u8], now_ms: u64) -> Result<Option<(bool, Vec<u8>)>> {
        if self.failed {
            return Err(Error::Sequence);
        }
        let result = (|| {
            if plain.len() < 12 {
                return Err(Error::Invalid);
            }
            self.check_timeout(now_ms)?;
            let word = u32::from_le_bytes(plain[..4].try_into().unwrap());
            let control = word >> 31 != 0;
            let id = word & 0x7fff_ffff;
            let total = u32::from_le_bytes(plain[4..8].try_into().unwrap()) as usize;
            let off = u32::from_le_bytes(plain[8..12].try_into().unwrap()) as usize;
            let data = &plain[12..];
            let cap = if control { 1024 } else { 4096 };
            if total == 0 || total > cap || data.is_empty() || off.checked_add(data.len()).is_none_or(|n| n > total) {
                return Err(Error::Limit);
            }
            let l = &mut self.lanes[control as usize];
            if !l.partial {
                if off != 0 || l.id.checked_add(1) != Some(id) || id == 0 {
                    return Err(Error::Sequence);
                }
                l.id = id;
                l.total = total;
                l.bytes.clear();
                l.partial = true;
            }
            if l.id != id || l.total != total || l.bytes.len() != off {
                return Err(Error::Sequence);
            }
            l.bytes.extend_from_slice(data);
            l.last_ms = now_ms;
            if l.bytes.len() == total {
                l.partial = false;
                Ok(Some((control, core::mem::take(&mut l.bytes))))
            } else {
                Ok(None)
            }
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn check_timeout(&mut self, now: u64) -> Result<()> {
        if self.failed || self.lanes.iter().any(|l| l.partial && now.saturating_sub(l.last_ms) > 2000) {
            self.failed = true;
            Err(Error::Sequence)
        } else {
            Ok(())
        }
    }
}
/// Diagnostic helper only. Never format production key material.
pub fn hex(bytes: &[u8]) -> String {
    use core::fmt::Write;
    let mut s = String::new();
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(v: &serde_json::Value) -> Vec<u8> {
        let s = v.as_str().unwrap();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
    #[test]
    fn independent_vectors() {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../protocol/vectors.json")).unwrap();
        let a = &v["auth"];
        let key: [u8; 32] = bytes(&a["key_hex"]).try_into().unwrap();
        let hn = bytes(&a["host_nonce_hex"]).try_into().unwrap();
        let bn = bytes(&a["board_nonce_hex"]).try_into().unwrap();
        let t = transcript(a["device_id"].as_str().unwrap(), &hn, &bn).unwrap();
        assert_eq!(hex(&t), a["transcript_hex"]);
        assert_eq!(hex(&auth_tag(&key, b"board", &t)), a["board_tag_hex"]);
        assert_eq!(hex(&auth_tag(&key, b"host", &t)), a["host_tag_hex"]);
        let k = Keys::derive(&key, &t);
        assert_eq!(hex(&k.host), a["derived"]["host-key"]);
        assert_eq!(hex(&k.board), a["derived"]["board-key"]);
        assert_eq!(hex(&k.epoch), a["derived"]["epoch"]);
        let mut c = Cipher::new(&k.host, k.host_nonce, 1);
        let wire = c.seal(0x80000001, 3, 0, b"abc").unwrap();
        assert_eq!(hex(&wire), v["fragment"]["wire_hex"]);
        let mut rx = Cipher::new(&k.host, k.host_nonce, 1);
        assert_eq!(hex(&rx.open(&wire).unwrap()), v["fragment"]["plaintext_hex"]);
        assert_eq!(rx.open(&wire), Err(Error::Counter));
        for r in v["requests"].as_array().unwrap() {
            let decisions: Vec<u8> =
                r["decisions"].as_array().unwrap().iter().map(|n| n.as_u64().unwrap() as u8).collect();
            let digest = request_digest(
                r["request_id"].as_str().unwrap(),
                r["session_id"].as_str().unwrap(),
                r["provider"].as_u64().unwrap() as u8,
                r["title"].as_str().unwrap(),
                r["tool_name"].as_str().unwrap(),
                &bytes(&r["body_sha256"]).try_into().unwrap(),
                r["body_length"].as_u64().unwrap() as u32,
                &decisions,
            )
            .unwrap();
            assert_eq!(hex(&digest), r["request_digest"]);
        }
    }
    #[test]
    fn corrupt_and_wrong_direction_fail_closed() {
        let key = [4; 32];
        let wire = Cipher::new(&key, [8; 4], 1).seal(1, 3, 0, b"abc").unwrap();
        for i in 0..wire.len() {
            let mut bad = wire.clone();
            bad[i] ^= 1;
            let mut rx = Cipher::new(&key, [8; 4], 1);
            assert!(rx.open(&bad).is_err());
            assert!(rx.open(&wire).is_err());
        }
        assert!(Cipher::new(&key, [8; 4], 2).open(&wire).is_err());
    }
    #[test]
    fn interleaved_lanes_and_timeout() {
        let key = [4; 32];
        let mut tx = Cipher::new(&key, [0; 4], 1);
        let mut rx = Cipher::new(&key, [0; 4], 1);
        let mut r = Reassembly::default();
        for (id, total, off, data, expected) in [
            (1, 6, 0, b"abc".as_slice(), None),
            (0x80000001, 2, 0, b"hi", Some((true, b"hi".to_vec()))),
            (1, 6, 3, b"def", Some((false, b"abcdef".to_vec()))),
        ] {
            let p = rx.open(&tx.seal(id, total, off, data).unwrap()).unwrap();
            assert_eq!(r.receive(&p, 10).unwrap(), expected);
        }
        let p = rx.open(&tx.seal(2, 4, 0, b"ab").unwrap()).unwrap();
        r.receive(&p, 20).unwrap();
        assert!(r.check_timeout(2021).is_err());
    }
}
