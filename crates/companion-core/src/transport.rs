//! Authenticated ordered fragments with bounded, independently scheduled lanes.
use crate::{
    codec,
    crypto::{Cipher, Error, Keys, Reassembly, Result},
    wire::Envelope,
};
use alloc::{collections::VecDeque, vec::Vec};
struct Message {
    id: u32,
    bytes: Vec<u8>,
    offset: usize,
}
pub struct Transport {
    tx: Cipher,
    rx: Cipher,
    reassembly: Reassembly,
    queues: [VecDeque<Message>; 2],
    ids: [u32; 2],
    capacity: usize,
    last_rx: u64,
    failed: bool,
}
impl Transport {
    pub fn new(keys: &Keys, host: bool, capacity: usize, now: u64) -> Result<Self> {
        if !(182..=509).contains(&capacity) {
            return Err(Error::Limit);
        }
        let h = || Cipher::new(&keys.host, keys.host_nonce, 1);
        let b = || Cipher::new(&keys.board, keys.board_nonce, 2);
        Ok(Self {
            tx: if host { h() } else { b() },
            rx: if host { b() } else { h() },
            reassembly: Reassembly::default(),
            queues: Default::default(),
            ids: [0; 2],
            capacity,
            last_rx: now,
            failed: false,
        })
    }
    pub fn enqueue(&mut self, e: &Envelope) -> Result<()> {
        if self.failed {
            return Err(Error::Sequence);
        }
        let bytes = codec::encode(e)?;
        let lane = codec::control(e.payload.as_ref().ok_or(Error::Invalid)?) as usize;
        if self.queues[lane].len() >= if lane == 0 { 16 } else { 32 } {
            return Err(Error::Limit);
        }
        let id = self.ids[lane].checked_add(1).filter(|id| *id < 0x80000000).ok_or(Error::Limit)?;
        self.ids[lane] = id;
        self.queues[lane].push_back(Message { id: id | ((lane as u32) << 31), bytes, offset: 0 });
        Ok(())
    }
    /// Call only when the platform can transmit; never pre-encrypt then reorder.
    pub fn next_fragment(&mut self) -> Result<Option<Vec<u8>>> {
        if self.failed {
            return Err(Error::Sequence);
        }
        let lane = if !self.queues[1].is_empty() { 1 } else { 0 };
        let Some(m) = self.queues[lane].front_mut() else { return Ok(None) };
        let end = (m.offset + self.capacity - 36).min(m.bytes.len());
        let packet = self.tx.seal(m.id, m.bytes.len() as u32, m.offset as u32, &m.bytes[m.offset..end])?;
        m.offset = end;
        if end == m.bytes.len() {
            self.queues[lane].pop_front();
        }
        Ok(Some(packet))
    }
    pub fn receive(&mut self, bytes: &[u8], now: u64) -> Result<Option<(Envelope, Vec<u8>)>> {
        let result = (|| {
            self.check(now)?;
            if bytes.len() > self.capacity {
                return Err(Error::Limit);
            }
            let plain = self.rx.open(bytes)?;
            self.last_rx = now;
            self.reassembly
                .receive(&plain, now)?
                .map(|(lane, raw)| codec::decode(&raw, lane).map(|e| (e, raw)))
                .transpose()
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn check(&mut self, now: u64) -> Result<()> {
        if self.failed || now.saturating_sub(self.last_rx) > 3000 || self.reassembly.check_timeout(now).is_err() {
            self.failed = true;
            return Err(Error::Sequence);
        }
        Ok(())
    }
    pub fn queued(&self) -> usize {
        self.queues.iter().map(VecDeque::len).sum()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{BlobChunk, Heartbeat, envelope::Payload};
    fn env(p: Payload) -> Envelope {
        Envelope { major: 2, connection_epoch: alloc::vec![1;16], authority_epoch: alloc::vec![2;16], payload: Some(p) }
    }
    #[test]
    fn control_preempts_bulk_at_every_supported_capacity() {
        for cap in [182, 244, 509] {
            let keys = Keys::derive(&[3; 32], b"fixture");
            let mut tx = Transport::new(&keys, true, cap, 0).unwrap();
            let mut rx = Transport::new(&keys, false, cap, 0).unwrap();
            tx.enqueue(&env(Payload::BlobChunk(BlobChunk {
                sha256: alloc::vec![4;32],
                data: alloc::vec![5;512],
                total_length: 512,
                r#final: true,
                ..Default::default()
            })))
            .unwrap();
            assert!(rx.receive(&tx.next_fragment().unwrap().unwrap(), 1).unwrap().is_none());
            tx.enqueue(&env(Payload::Heartbeat(Heartbeat::default()))).unwrap();
            let mut seen = alloc::vec![];
            while let Some(f) = tx.next_fragment().unwrap() {
                if let Some((e, _)) = rx.receive(&f, 2).unwrap() {
                    seen.push(e.payload.unwrap());
                }
            }
            assert!(matches!(seen[0], Payload::Heartbeat(_)));
            assert!(matches!(seen[1], Payload::BlobChunk(_)));
        }
    }
    #[test]
    fn rejects_small_mtu_and_idle_connection() {
        let k = Keys::derive(&[0; 32], b"t");
        assert!(Transport::new(&k, true, 64, 0).is_err());
        assert!(Transport::new(&k, true, 182, 0).unwrap().check(3001).is_err());
    }
}
