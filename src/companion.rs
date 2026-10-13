//! BLE companion transport. Only completed, checksum-verified frames become the
//! delta baseline. One latest-frame slot prevents slow radio links queuing UI.
use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification, WriteType};
use btleplug::platform::{Manager, Peripheral};
use futures::{Stream, StreamExt};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io::Read;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 320;
pub const FRAME_BYTES: usize = WIDTH * HEIGHT * 2;
pub const SERVICE: Uuid = Uuid::from_u128(0x7e57cafe_0001_4000_8000_000000000001);
const RX: Uuid = Uuid::from_u128(0x7e57cafe_0001_4000_8000_000000000002);
const AUTH: Uuid = Uuid::from_u128(0x7e57cafe_0001_4000_8000_000000000004);
const TX: Uuid = Uuid::from_u128(0x7e57cafe_0001_4000_8000_000000000003);
pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub fn read_key(path: &str) -> Result<[u8; 32], Error> {
    let text = std::fs::read_to_string(path)?;
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return Err("Pairing key must be 64 hex characters".into());
    }
    let mut key = [0; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)?;
    }
    Ok(key)
}
fn auth_tag(key: &[u8; 32], role: &[u8], challenge: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC key length");
    mac.update(role);
    mac.update(challenge);
    mac.finalize().into_bytes()[..16].to_vec()
}

pub fn encode_frame(pixels: &[u32]) -> Vec<u8> {
    assert_eq!(pixels.len(), 320 * 240);
    let mut out = vec![0; FRAME_BYTES];
    for y in 0..240 {
        for x in 0..320 {
            let p = pixels[y * 320 + x];
            let color = ((((p >> 19) & 31) << 11) | (((p >> 10) & 63) << 5) | ((p >> 3) & 31)) as u16;
            let offset = ((y + 40) * WIDTH + x + 80) * 2;
            out[offset..offset + 2].copy_from_slice(&color.to_le_bytes());
        }
    }
    out
}

pub fn packet(op: u8, seq: u32, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 4096);
    let mut header = b"TCF1".to_vec();
    header.extend([op, 1]);
    header.extend((payload.len() as u16).to_le_bytes());
    header.extend(seq.to_le_bytes());
    let mut hash = crc32fast::Hasher::new();
    hash.update(&header);
    hash.update(payload);
    header.extend(hash.finalize().to_le_bytes());
    header.extend(payload);
    header
}

/// Four-row stripes bound worst-case raw data to the firmware payload limit.
fn rectangle(frame: &[u8], y: usize) -> (u8, Vec<u8>) {
    let mut head = Vec::new();
    for n in [0, y as u16, WIDTH as u16, 4] {
        head.extend(n.to_le_bytes());
    }
    let pixels = &frame[y * WIDTH * 2..(y + 4) * WIDTH * 2];
    let mut rle = head.clone();
    let mut i = 0;
    while i < pixels.len() {
        let color = &pixels[i..i + 2];
        let mut count = 1u16;
        while i + (count as usize + 1) * 2 <= pixels.len()
            && &pixels[i + count as usize * 2..i + count as usize * 2 + 2] == color
        {
            count += 1;
        }
        rle.extend(count.to_le_bytes());
        rle.extend(color);
        i += count as usize * 2;
    }
    if rle.len() < head.len() + pixels.len() {
        (6, rle)
    } else {
        head.extend(pixels);
        (2, head)
    }
}

pub struct Link {
    peripheral: Peripheral,
    rx: Characteristic,
    notifications: Pin<Box<dyn Stream<Item = ValueNotification> + Send>>,
    sequence: u32,
    chunk_size: usize,
    baseline: Option<Vec<u8>>,
    pub name: String,
}
impl Link {
    /// Refuse ambiguous discovery; optionally restrict to a previously chosen identity.
    pub async fn connect(identity: Option<&str>, key: &[u8; 32]) -> Result<Self, Error> {
        eprintln!("Bluetooth: initializing adapter");
        let manager = Manager::new().await?;
        let adapter = manager.adapters().await?.into_iter().next().ok_or("No Bluetooth adapter")?;
        adapter.start_scan(ScanFilter { services: vec![SERVICE] }).await?;
        eprintln!("Bluetooth: scanning");
        sleep(Duration::from_secs(4)).await;
        let mut matches = Vec::new();
        for p in adapter.peripherals().await? {
            if let Some(props) = p.properties().await?
                && props.services.contains(&SERVICE)
                && identity.is_none_or(|id| props.local_name.as_deref() == Some(id))
            {
                matches.push((p, props.local_name.unwrap_or_default()));
            }
        }
        adapter.stop_scan().await?;
        if matches.len() != 1 {
            return Err(format!("Expected one TokenCafe board, found {}", matches.len()).into());
        }
        let (peripheral, name) = matches.remove(0);
        eprintln!("Bluetooth: opening {name}");
        timeout(Duration::from_secs(15), peripheral.connect()).await??;
        let setup = async {
            peripheral.discover_services().await?;
            eprintln!("Bluetooth: verifying board identity");
            let chars = peripheral.characteristics();
            let auth = chars.iter().find(|c| c.uuid == AUTH).ok_or("Board lacks authentication")?;
            let mut challenge = [0u8; 16];
            std::fs::File::open("/dev/urandom")?.read_exact(&mut challenge)?;
            peripheral.write(auth, &challenge, WriteType::WithResponse).await?;
            let response = peripheral.read(auth).await?;
            if response.len() != 32 {
                return Err("Invalid board authentication response".into());
            }
            let challenge = [challenge.as_slice(), &response[..16]].concat();
            let expected = auth_tag(key, b"board", &challenge);
            let difference = expected.iter().zip(&response[16..]).fold(0u8, |diff, (a, b)| diff | (a ^ b));
            if difference != 0 {
                return Err("Board authentication failed; no frame data sent".into());
            }
            peripheral.write(auth, &auth_tag(key, b"host", &challenge), WriteType::WithResponse).await?;
            let rx = chars.iter().find(|c| c.uuid == RX).ok_or("Missing companion RX")?.clone();
            let tx = chars.iter().find(|c| c.uuid == TX).ok_or("Missing companion TX")?.clone();
            eprintln!("Bluetooth: board authentication verified");
            let caps = peripheral.read(auth).await?;
            if caps.len() != 2 {
                return Err("Invalid BLE capacity response".into());
            }
            let chunk_size = u16::from_le_bytes([caps[0], caps[1]]) as usize;
            if !(20..=512).contains(&chunk_size) {
                return Err("Invalid negotiated BLE payload".into());
            }
            eprintln!("Bluetooth: negotiated {chunk_size}-byte writes");
            let notifications = peripheral.notifications().await?;
            peripheral.subscribe(&tx).await?;
            Ok::<_, Error>((rx, notifications, chunk_size))
        }
        .await;
        let (rx, notifications, chunk_size) = match setup {
            Ok(v) => v,
            Err(e) => {
                let _ = timeout(Duration::from_secs(2), peripheral.disconnect()).await;
                return Err(e);
            }
        };
        let mut link = Self { peripheral, rx, notifications, sequence: 0, chunk_size, baseline: None, name };
        let hello = link.request(1, &[], 0).await;
        if !matches!(&hello, Ok(v) if v == &[0xe0, 1, 0x40, 1, 0, 0x10, 1]) {
            link.disconnect().await;
            return Err("Invalid companion handshake".into());
        }
        Ok(link)
    }
    pub async fn disconnect(&self) {
        let _ = timeout(Duration::from_secs(2), self.peripheral.disconnect()).await;
    }
    pub async fn request(&mut self, op: u8, payload: &[u8], status: u8) -> Result<Vec<u8>, Error> {
        self.sequence = self.sequence.wrapping_add(1);
        let msg = packet(op, self.sequence, payload);
        // Use the actual server MTU after authentication, including default-MTU links.
        for chunk in msg.chunks(self.chunk_size) {
            timeout(Duration::from_secs(5), self.peripheral.write(&self.rx, chunk, WriteType::WithResponse)).await??;
        }
        let seq = self.sequence;
        let reply = timeout(Duration::from_secs(10), async {
            let mut bytes = Vec::new();
            loop {
                let n = self.notifications.next().await.ok_or("Bluetooth notification stream closed")?;
                if n.uuid != TX {
                    continue;
                }
                bytes.extend(n.value);
                if bytes.len() < 16 {
                    continue;
                }
                if &bytes[..4] != b"TCF1" {
                    return Err::<Vec<u8>, Error>("Invalid response magic".into());
                }
                let len = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
                if !(1..=16).contains(&len) {
                    return Err("Invalid response length".into());
                }
                if bytes.len() < 16 + len {
                    continue;
                }
                let mut hasher = crc32fast::Hasher::new();
                hasher.update(&bytes[..12]);
                hasher.update(&bytes[16..]);
                if bytes.len() != 16 + len
                    || bytes[4] != op | 0x80
                    || bytes[5] != 1
                    || u32::from_le_bytes(bytes[8..12].try_into()?) != seq
                    || u32::from_le_bytes(bytes[12..16].try_into()?) != hasher.finalize()
                    || bytes[16] != status
                {
                    return Err("Invalid response status/sequence/CRC".into());
                }
                return Ok(bytes[17..].to_vec());
            }
        })
        .await??;
        Ok(reply)
    }
    pub async fn upload(&mut self, frame: &[u8]) -> Result<usize, Error> {
        if frame.len() != FRAME_BYTES {
            return Err("Invalid frame dimensions".into());
        }
        if self.baseline.is_none() {
            self.request(5, &[], 0).await?;
        }
        let xor = self.baseline.as_ref().map(|b| frame.iter().zip(b).map(|(a, b)| a ^ b).collect::<Vec<u8>>());
        let mut sent = 0;
        for y in (0..HEIGHT).step_by(4) {
            let range = y * WIDTH * 2..(y + 4) * WIDTH * 2;
            if self.baseline.as_ref().is_some_and(|b| b[range.clone()] == frame[range]) {
                continue;
            }
            let (mut op, mut rect) = rectangle(frame, y);
            if let Some(delta) = &xor {
                let (encoding, data) = rectangle(delta, y);
                if encoding == 6 && data.len() < rect.len() {
                    op = 8;
                    rect = data;
                }
            }
            sent += rect.len() + 16;
            if let Err(e) = self.request(op, &rect, 0).await {
                self.baseline = None;
                return Err(e);
            }
        }
        let reply = self.request(3, &[], 0).await;
        match reply {
            Ok(crc) if crc == crc32fast::hash(frame).to_le_bytes() => {
                self.baseline = Some(frame.to_vec());
                Ok(sent)
            }
            _ => {
                self.baseline = None;
                Err("Companion frame CRC mismatch or disconnected".into())
            }
        }
    }
}

pub struct Mirror {
    pending: Arc<Mutex<Option<Vec<u8>>>>,
    last: Instant,
    status: Arc<Mutex<String>>,
}
impl Mirror {
    pub fn start(key: [u8; 32]) -> Self {
        let pending = Arc::new(Mutex::new(None::<Vec<u8>>));
        let input = pending.clone();
        let status = Arc::new(Mutex::new("searching".to_string()));
        let progress = status.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("Bluetooth runtime");
            runtime.block_on(async move {
                let mut identity = None;
                loop {
                    *progress.lock().unwrap() = "searching".into();
                    eprintln!("Bluetooth: searching for companion…");
                    match Link::connect(identity.as_deref(), &key).await {
                        Ok(mut link) => {
                            identity = Some(link.name.clone());
                            *progress.lock().unwrap() = format!("{} syncing", link.name);
                            eprintln!("Bluetooth: connected to {}", link.name);
                            loop {
                                let frame = input.lock().unwrap().take();
                                if let Some(frame) = frame {
                                    let start = Instant::now();
                                    match link.upload(&frame).await {
                                        Ok(n) => {
                                            *progress.lock().unwrap() = format!("{} mirrored", link.name);
                                            eprintln!(
                                                "Bluetooth: verified frame CRC={:08x}, {n} bytes, {:.2}s",
                                                crc32fast::hash(&frame),
                                                start.elapsed().as_secs_f32()
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!("Bluetooth: {e}; reconnecting");
                                            break;
                                        }
                                    }
                                } else {
                                    sleep(Duration::from_millis(100)).await;
                                }
                            }
                            link.disconnect().await;
                        }
                        Err(e) => {
                            *progress.lock().unwrap() = "unavailable; retrying".into();
                            eprintln!("Bluetooth: {e}");
                        }
                    }
                    sleep(Duration::from_secs(3)).await;
                }
            });
        });
        Self { pending, status, last: Instant::now() - Duration::from_secs(1) }
    }
    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }
    pub fn publish(&mut self, pixels: &[u32]) {
        if self.last.elapsed() >= Duration::from_millis(500) {
            *self.pending.lock().unwrap() = Some(encode_frame(pixels));
            self.last = Instant::now();
        }
    }
}

/// Physical-board acceptance test, using the same transport as the desktop UI.
pub fn hardware_test(path: &str, key: &[u8; 32]) -> Result<(), Error> {
    let frame = std::fs::read(path)?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let mut wrong_key = *key;
        wrong_key[0] ^= 1;
        match Link::connect(None, &wrong_key).await {
            Err(e) if e.to_string().contains("Board authentication failed") => {
                eprintln!("PASS wrong pairing key rejected before frame transfer")
            }
            Ok(link) => {
                link.disconnect().await;
                return Err("Wrong key was accepted".into());
            }
            Err(e) => return Err(e),
        }
        sleep(Duration::from_secs(2)).await;
        let mut link = Link::connect(None, key).await?;
        eprintln!("TEST connected {}", link.name);
        let name = link.name.clone();
        let result = async {
            let start = Instant::now();
            let size = link.upload(&frame).await?;
            eprintln!(
                "PASS full frame {size} bytes in {:.2}s CRC={:08x}",
                start.elapsed().as_secs_f32(),
                crc32fast::hash(&frame)
            );
            let mut changed = frame.clone();
            changed[1000..1002].copy_from_slice(&[0x34, 0x12]);
            let size = link.upload(&changed).await?;
            eprintln!("PASS delta {size} bytes");
            // An RLE count larger than the rectangle must not mutate the staging frame.
            let invalid = [0, 0, 0, 0, 1, 0, 1, 0, 2, 0, 0xff, 0xff];
            link.request(6, &invalid, 1).await?;
            let crc = link.request(3, &[], 0).await?;
            if crc != crc32fast::hash(&changed).to_le_bytes() {
                return Err::<(), Error>("Invalid RLE mutated frame".into());
            }
            eprintln!("PASS invalid RLE rejected without frame mutation");
            Ok(())
        }
        .await;
        link.disconnect().await;
        result?;
        sleep(Duration::from_secs(2)).await;
        let mut second = Link::connect(Some(&name), key).await?;
        let result = second.upload(&frame).await;
        second.disconnect().await;
        result?;
        eprintln!("PASS reconnect and full resynchronization");
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_positions_and_color() {
        let image = encode_frame(&vec![0xff0000; 320 * 240]);
        assert_eq!(&image[((40 * WIDTH + 80) * 2)..][..2], &[0, 0xf8]);
        assert!(image[..40 * WIDTH * 2].iter().all(|v| *v == 0));
        assert_eq!(image.len(), FRAME_BYTES);
    }
    #[test]
    fn stripes_round_trip_and_raw_fallback() {
        for image in [vec![0x45; FRAME_BYTES], (0..FRAME_BYTES).map(|i| (i % 251) as u8).collect()] {
            for y in (0..HEIGHT).step_by(4) {
                let (op, data) = rectangle(&image, y);
                assert!(data.len() <= 4096);
                let mut decoded: Vec<u8> = Vec::new();
                if op == 6 {
                    for run in data[8..].as_chunks::<4>().0 {
                        for _ in 0..u16::from_le_bytes([run[0], run[1]]) {
                            decoded.extend(&run[2..]);
                        }
                    }
                } else {
                    decoded.extend(&data[8..]);
                }
                assert_eq!(decoded, image[y * WIDTH * 2..(y + 4) * WIDTH * 2]);
            }
        }
    }
}
