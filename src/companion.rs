//! BLE companion transport. Only completed, checksum-verified frames become the
//! delta baseline. One latest-frame slot prevents slow radio links queuing UI.
use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification, WriteType,
};
use btleplug::platform::{Manager, Peripheral};
use futures::{Stream, StreamExt};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::VecDeque;
use std::io::Read;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

mod codec;
mod reply;
use codec::updates;
pub use codec::{encode_frame, packet};
use reply::RemoteStatus;

pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 272;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Tap(i32, i32),
    DoubleTap(i32, i32),
    Scroll(i32),
}
impl Input {
    pub fn trace(self, scene: &crate::scene::Scene) {
        if std::env::var_os("TC_TOUCH_QA").is_some() {
            let (kind, x, y) = match self {
                Input::Tap(x, y) => ("tap", x, y),
                Input::DoubleTap(x, y) => ("double", x, y),
                Input::Scroll(y) => ("scroll", 470, y),
            };
            let hit = scene.patron_at(x, y);
            let target = hit.and_then(|id| scene.session_target(id)).and_then(|t| t.url());
            let seats: Vec<_> = (0..8).map(|i| {
                        let id = scene.patron_at(60+(i%4)*120,80+(i/4)*88);
                        serde_json::json!({"seat":i,"id":id,"target":id.and_then(|id| scene.session_target(id)).and_then(|t| t.url())})
                    }).collect();
            eprintln!(
                "TOUCH_QA {}",
                serde_json::json!({"kind":kind,"x":x,"y":y,"hit":hit,"target":target,"seats":seats})
            );
        }
    }

    /// Shared by the native app and the hardware integration test host.
    pub fn apply(self, scene: &mut crate::scene::Scene) -> Option<String> {
        match self {
            Self::Tap(x, y) => scene.click(x, y),
            Self::DoubleTap(x, y) => {
                if scene.patron_at(x, y).is_some() {
                    return scene.activate_patron(x, y);
                }
                scene.click(x, y);
            }
            Self::Scroll(y) => {
                if !scene.scrollbar_hit(470, 252) {
                    scene.select_next();
                }
                scene.scroll_task_to(y);
            }
        }
        None
    }
}
fn decode_input(data: &[u8]) -> Result<Option<Input>, Error> {
    if data.is_empty() {
        return Ok(None);
    }
    if data.len() != 5 {
        return Err("Invalid companion input length".into());
    }
    let x = u16::from_le_bytes([data[1], data[2]]) as usize;
    let y = u16::from_le_bytes([data[3], data[4]]) as usize;
    if x >= WIDTH || y >= HEIGHT {
        return Err("Invalid companion input coordinates".into());
    }
    let x = x as i32;
    let y = y as i32;
    match data[0] {
        1 if (0..480).contains(&x) && (0..272).contains(&y) => Ok(Some(Input::Tap(x, y))),
        1 => Ok(None),
        2 => Ok(Some(Input::Scroll(y.clamp(224, 271)))),
        3 => Ok(Some(Input::DoubleTap(x, y))),
        _ => Err("Invalid companion input kind".into()),
    }
}
type InputQueue = Arc<Mutex<VecDeque<Input>>>;
pub struct Link {
    peripheral: Peripheral,
    rx: Characteristic,
    notifications: Pin<Box<dyn Stream<Item = ValueNotification> + Send>>,
    sequence: u32,
    chunk_size: usize,
    baseline: Option<Vec<u8>>,
    pub name: String,
    input: Option<InputQueue>,
    last_input_poll: Instant,
    fast: bool,
}
impl Link {
    /// Refuse ambiguous discovery; optionally restrict to a previously chosen identity.
    pub async fn connect(identity: Option<&str>, key: &[u8; 32]) -> Result<Self, Error> {
        eprintln!("Bluetooth: initializing adapter");
        let manager = timeout(Duration::from_secs(5), Manager::new()).await??;
        let adapter = timeout(Duration::from_secs(5), manager.adapters())
            .await??
            .into_iter()
            .next()
            .ok_or("No Bluetooth adapter")?;
        let discovery = timeout(Duration::from_secs(15), async {
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
            Ok::<_, Error>(matches)
        })
        .await;
        let stopped = timeout(Duration::from_secs(2), adapter.stop_scan()).await;
        let mut matches = discovery??;
        stopped??;
        if matches.len() != 1 {
            return Err(format!("Expected one TokenCafe board, found {}", matches.len()).into());
        }
        let (peripheral, name) = matches.remove(0);
        eprintln!("Bluetooth: opening {name}");
        let connected = timeout(Duration::from_secs(15), peripheral.connect()).await;
        if !matches!(&connected, Ok(Ok(()))) {
            let _ = timeout(Duration::from_secs(2), peripheral.disconnect()).await;
            connected??;
        }
        let setup = timeout(Duration::from_secs(30), async {
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
        })
        .await
        .unwrap_or_else(|_| Err("Bluetooth setup timed out".into()));
        let (rx, notifications, chunk_size) = match setup {
            Ok(v) => v,
            Err(e) => {
                let _ = timeout(Duration::from_secs(2), peripheral.disconnect()).await;
                return Err(e);
            }
        };
        let mut link = Self {
            peripheral,
            rx,
            notifications,
            sequence: 0,
            chunk_size,
            baseline: None,
            name,
            input: None,
            fast: false,
            last_input_poll: Instant::now() - Duration::from_secs(1),
        };
        let hello = link.request(1, &[], 0).await;
        let mut expected = Vec::new();
        expected.extend((WIDTH as u16).to_le_bytes());
        expected.extend((HEIGHT as u16).to_le_bytes());
        expected.extend([0, 0x10, 1]);
        if !matches!(&hello, Ok(v) if v == &expected) {
            link.disconnect().await;
            return Err("Companion dimensions or protocol mismatch; rebuild and flash matching firmware".into());
        }
        // Unknown opcode on older firmware safely leaves the legacy path enabled.
        link.fast = match link.request(12, &[], 0).await {
            Ok(caps) => caps == [3, 1],
            Err(e) if e.downcast_ref::<RemoteStatus>().is_some_and(|s| s.0 == 3) => false,
            Err(e) => {
                link.disconnect().await;
                return Err(e);
            }
        };
        eprintln!("Bluetooth: interruptible compressed protocol={}", link.fast);
        Ok(link)
    }
    pub async fn disconnect(&self) {
        let _ = timeout(Duration::from_secs(2), self.peripheral.disconnect()).await;
    }
    pub async fn request(&mut self, op: u8, payload: &[u8], status: u8) -> Result<Vec<u8>, Error> {
        self.sequence = self.sequence.wrapping_add(1);
        let msg = packet(op, self.sequence, payload);
        // Use the actual server MTU after authentication, including default-MTU links.
        let chunks = msg.chunks(self.chunk_size);
        for (index, chunk) in chunks.enumerate() {
            // A response every eight fragments bounds the native send queue.
            // The final protocol acknowledgment is the packet boundary barrier.
            let write = if self.rx.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) && index % 8 != 7 {
                WriteType::WithoutResponse
            } else {
                WriteType::WithResponse
            };
            timeout(Duration::from_secs(5), self.peripheral.write(&self.rx, chunk, write)).await??;
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
                if let Some(reply) = reply::decode(&bytes, op, seq, status)? {
                    return Ok::<_, Error>(reply);
                }
            }
        })
        .await??;
        Ok(reply)
    }
    fn receive_input(&self, bytes: &[u8]) -> Result<bool, Error> {
        if let Some(event) = decode_input(bytes)?
            && let Some(queue) = &self.input
        {
            let mut q = queue.lock().unwrap();
            if q.len() >= 32 {
                q.pop_front();
            }
            q.push_back(event);
            return Ok(true);
        }
        Ok(false)
    }
    async fn poll_input(&mut self) -> Result<bool, Error> {
        if self.input.is_none() {
            return Ok(false);
        }
        if self.last_input_poll.elapsed() < Duration::from_millis(75) {
            return Ok(false);
        }
        self.last_input_poll = Instant::now();
        let mut received = false;
        for _ in 0..8 {
            let bytes = self.request(9, &[], 0).await?;
            if bytes.is_empty() {
                break;
            }
            received |= self.receive_input(&bytes)?;
        }
        Ok(received)
    }
    pub async fn upload(&mut self, frame: &[u8]) -> Result<usize, Error> {
        self.upload_frame(frame, None).await?.ok_or_else(|| "Unexpected preemption".into())
    }
    async fn preempt(&mut self, pending: Option<(&PendingFrame, u64)>, received: bool) -> Result<bool, Error> {
        if !self.fast {
            return Ok(false);
        }
        if let Some((slot, generation)) = pending {
            // Give the UI one render tick to apply the just-delivered input.
            if received {
                sleep(Duration::from_millis(40)).await;
            }
            let newer = slot.lock().unwrap().as_ref().is_some_and(|(_, next)| *next > generation);
            if newer {
                self.request(11, &[], 0).await?;
                eprintln!("Bluetooth: preempted background frame for touch");
                return Ok(true);
            }
        }
        Ok(false)
    }
    async fn upload_frame(
        &mut self,
        frame: &[u8],
        pending: Option<(&PendingFrame, u64)>,
    ) -> Result<Option<usize>, Error> {
        if frame.len() != FRAME_BYTES {
            return Err("Invalid frame dimensions".into());
        }
        if self.baseline.is_none() {
            self.request(5, &[], 0).await?;
        }
        let mut sent = 0;
        let packets = updates(frame, self.baseline.as_deref(), self.fast);
        let mut received = if !self.fast || packets.is_empty() { self.poll_input().await? } else { false };
        for (op, rect) in packets {
            if self.preempt(pending, received).await? {
                return Ok(None);
            }
            received = false;
            // LZ4 acknowledgments already carry input, avoiding a separate poll RTT.
            if !self.fast {
                received = self.poll_input().await?;
            }
            if self.preempt(pending, received).await? {
                return Ok(None);
            }
            sent += rect.len() + 16;
            match self.request(op, &rect, 0).await {
                Ok(bytes) => {
                    if self.fast {
                        received = self.receive_input(&bytes)?;
                    }
                }
                Err(e) => {
                    self.baseline = None;
                    return Err(e);
                }
            }
        }
        if self.preempt(pending, received).await? {
            return Ok(None);
        }
        let reply = self.request(3, &[], 0).await;
        match reply {
            Ok(crc) if crc == crc32fast::hash(frame).to_le_bytes() => {
                self.baseline = Some(frame.to_vec());
                Ok(Some(sent))
            }
            _ => {
                self.baseline = None;
                Err("Companion frame CRC mismatch or disconnected".into())
            }
        }
    }
}

type PendingFrame = Arc<Mutex<Option<(Vec<u8>, u64)>>>;

pub struct Mirror {
    input: InputQueue,
    pending: PendingFrame,
    input_generation: u64,
    last: Instant,
    status: Arc<Mutex<String>>,
}
impl Mirror {
    pub fn start(key: [u8; 32]) -> Self {
        let inputs: InputQueue = Arc::new(Mutex::new(VecDeque::new()));
        let thread_inputs = inputs.clone();
        let pending = Arc::new(Mutex::new(None::<(Vec<u8>, u64)>));
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
                            thread_inputs.lock().unwrap().clear();
                            link.input = Some(thread_inputs.clone());
                            identity = Some(link.name.clone());
                            *progress.lock().unwrap() = format!("{} syncing", link.name);
                            eprintln!("Bluetooth: connected to {}", link.name);
                            loop {
                                let frame = input.lock().unwrap().take();
                                if let Some((frame, generation)) = frame {
                                    let start = Instant::now();
                                    match link.upload_frame(&frame, Some((&input, generation))).await {
                                        Ok(Some(n)) => {
                                            *progress.lock().unwrap() = format!("{} mirrored", link.name);
                                            eprintln!(
                                                "Bluetooth: verified frame CRC={:08x}, {n} bytes, {:.2}s input={generation}",
                                                crc32fast::hash(&frame),
                                                start.elapsed().as_secs_f32()
                                            );
                                        }
                                        Ok(None) => continue,
                                        Err(e) => {
                                            eprintln!("Bluetooth: {e}; reconnecting");
                                            break;
                                        }
                                    }
                                } else {
                                    if link.poll_input().await.is_err() {
                                        break;
                                    }
                                    sleep(Duration::from_millis(20)).await;
                                }
                            }
                            // Invalidate undelivered input immediately on failure, not only
                            // after a later connection succeeds.
                            thread_inputs.lock().unwrap().clear();
                            *progress.lock().unwrap() = "disconnected; retrying".into();
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
        Self { input: inputs, pending, input_generation: 0, status, last: Instant::now() - Duration::from_secs(1) }
    }
    pub fn drain_input(&mut self) -> Vec<Input> {
        let events: Vec<_> = self.input.lock().unwrap().drain(..).collect();
        if !events.is_empty() {
            self.last = Instant::now() - Duration::from_secs(1);
        }
        for event in &events {
            self.input_generation += 1;
            eprintln!("Bluetooth: applied input={} {event:?}", self.input_generation);
        }
        events
    }
    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }
    pub fn publish(&mut self, pixels: &[u32]) {
        if self.last.elapsed() >= Duration::from_millis(100) {
            *self.pending.lock().unwrap() = Some((encode_frame(pixels), self.input_generation));
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
            if link.fast {
                let update = [0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0xff, 0xff];
                link.request(6, &update, 0).await?;
                let pending = Arc::new(Mutex::new(Some((frame.clone(), 1))));
                if !link.preempt(Some((&pending, 0)), false).await? {
                    return Err("New touch failed to preempt background update".into());
                }
                if link.request(3, &[], 0).await? != crc32fast::hash(&changed).to_le_bytes() {
                    return Err("Aborted frame did not restore committed baseline".into());
                }
                eprintln!("PASS newer touch preempted update and restored committed framebuffer");
            }
            if let Some(dir) = std::env::var_os("TC_BLE_BENCH_DIR") {
                let dir = std::path::PathBuf::from(dir);
                for name in [
                    "03-patron-selected",
                    "04-down-second-page",
                    "09-banner-next-patron",
                    "10-left-previous",
                    "11-toggle-off",
                ] {
                    let next = std::fs::read(dir.join(format!("{name}.rgb565")))?;
                    let start = Instant::now();
                    let size = link.upload(&next).await?;
                    eprintln!("BENCH {name}: {size} bytes {:.3}s", start.elapsed().as_secs_f64());
                }
            }
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
    use super::codec::rectangle;
    use super::*;
    #[test]
    fn touch_decode_checks_bounds_and_maps_full_panel() {
        assert_eq!(decode_input(&[]).unwrap(), None);
        assert_eq!(decode_input(&[3, 60, 0, 91, 0]).unwrap(), Some(Input::DoubleTap(60, 91)));
        assert!(decode_input(&[3, 224, 1, 0, 0]).is_err());
        assert_eq!(decode_input(&[1, 0, 0, 0, 0]).unwrap(), Some(Input::Tap(0, 0)));
        assert_eq!(decode_input(&[1, 223, 1, 15, 1]).unwrap(), Some(Input::Tap(479, 271)));
        assert_eq!(decode_input(&[1, 240, 0, 136, 0]).unwrap(), Some(Input::Tap(240, 136)));
        assert_eq!(decode_input(&[2, 134, 1, 15, 1]).unwrap(), Some(Input::Scroll(271)));
        assert!(decode_input(&[1, 224, 1, 0, 0]).is_err());
        assert!(decode_input(&[1, 80, 0, 16, 1]).is_err());
        assert!(decode_input(&[9, 80, 0, 16, 0]).is_err());
        assert!(decode_input(&[1, 80]).is_err());
    }
    #[test]
    fn every_panel_pixel_maps_to_the_pixel_rendered_there() {
        let pixels: Vec<u32> = (0..WIDTH * HEIGHT).map(|i| (i as u32).wrapping_mul(2654435761) & 0xffffff).collect();
        let frame = encode_frame(&pixels);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let data = [1, x as u8, (x >> 8) as u8, y as u8, (y >> 8) as u8];
                let Some(Input::Tap(sx, sy)) = decode_input(&data).unwrap() else { panic!("tap lost") };
                assert_eq!((sx, sy), (x as i32, y as i32));
                let p = pixels[sy as usize * WIDTH + sx as usize];
                let expected = ((((p >> 19) & 31) << 11) | (((p >> 10) & 63) << 5) | ((p >> 3) & 31)) as u16;
                let at = (y * WIDTH + x) * 2;
                assert_eq!(u16::from_le_bytes([frame[at], frame[at + 1]]), expected);
            }
        }
    }

    #[test]
    fn frame_positions_and_color() {
        let image = encode_frame(&vec![0xff0000; WIDTH * HEIGHT]);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let expected = [0, 0xf8];
                assert_eq!(&image[(y * WIDTH + x) * 2..][..2], &expected);
            }
        }
        assert_eq!(image.len(), FRAME_BYTES);
    }
    #[test]
    fn planned_updates_reconstruct_frames_and_skip_unchanged() {
        let baseline = vec![0x45; FRAME_BYTES];
        let mut sparse = baseline.clone();
        sparse[100..104].copy_from_slice(&[1, 2, 3, 4]);
        sparse[FRAME_BYTES - 2..].copy_from_slice(&[5, 6]);
        let noisy: Vec<u8> = (0..FRAME_BYTES).map(|i| (i % 251) as u8).collect();
        for (frame, previous) in
            [(&baseline, None), (&sparse, Some(baseline.as_slice())), (&noisy, Some(baseline.as_slice()))]
        {
            let mut actual = previous.map_or_else(|| vec![0; FRAME_BYTES], |p| p.to_vec());
            for (op, data) in updates(frame, previous, false) {
                assert!(data.len() <= 4096);
                let y = u16::from_le_bytes([data[2], data[3]]) as usize;
                let height = u16::from_le_bytes([data[6], data[7]]) as usize;
                let mut decoded = Vec::new();
                if op == 2 {
                    decoded.extend_from_slice(&data[8..]);
                } else {
                    for run in data[8..].as_chunks::<4>().0 {
                        for _ in 0..u16::from_le_bytes([run[0], run[1]]) {
                            decoded.extend_from_slice(&run[2..]);
                        }
                    }
                }
                assert_eq!(decoded.len(), height * WIDTH * 2);
                for (dst, value) in actual[y * WIDTH * 2..(y + height) * WIDTH * 2].iter_mut().zip(decoded) {
                    if op == 8 {
                        *dst ^= value;
                    } else {
                        *dst = value;
                    }
                }
            }
            assert_eq!(&actual, frame);
        }
        assert!(updates(&baseline, Some(&baseline), false).is_empty());
        assert!(updates(&sparse, Some(&baseline), false).len() <= 2);
    }

    #[test]
    fn fast_protocol_cross_language_fixtures() {
        // Optional fixture export is consumed by the actual C decoder under ASan.
        let Some(dir) = std::env::var_os("TC_CODEC_FIXTURES") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut state = 19u32;
        let mut frames = vec![
            vec![0x45; FRAME_BYTES],
            (0..FRAME_BYTES)
                .map(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    (state >> 24) as u8
                })
                .collect(),
        ];
        if let Ok(entries) = std::fs::read_dir("target/companion-qa") {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == "rgb565") {
                    frames.push(std::fs::read(entry.path()).unwrap());
                }
            }
        }
        for (index, frame) in frames.iter().enumerate() {
            let mut stream = packet(5, 1, &[]);
            let packets = updates(frame, None, true);
            for (seq, (op, payload)) in packets.iter().enumerate() {
                assert!(payload.len() <= 4096);
                stream.extend(packet(*op, seq as u32 + 2, payload));
            }
            if index > 0 {
                let mut transition = packet(5, 1, &[]);
                for (op, payload) in updates(&frames[index - 1], None, true) {
                    transition.extend(packet(op, 2, &payload));
                }
                let delta = updates(frame, Some(&frames[index - 1]), true);
                eprintln!("transition {index}: {} bytes", delta.iter().map(|(_, p)| p.len() + 16).sum::<usize>());
                for (op, payload) in delta {
                    transition.extend(packet(op, 3, &payload));
                }
                std::fs::write(dir.join(format!("transition-{index}.packets")), transition).unwrap();
                std::fs::write(dir.join(format!("transition-{index}.rgb565")), frame).unwrap();
            }
            std::fs::write(dir.join(format!("{index}.packets")), &stream).unwrap();
            std::fs::write(dir.join(format!("{index}.rgb565")), frame).unwrap();
            eprintln!("fixture {index}: {} bytes / {} packets", stream.len(), packets.len());
        }
    }

    #[test]
    fn stripes_round_trip_and_raw_fallback() {
        for image in [vec![0x45; FRAME_BYTES], (0..FRAME_BYTES).map(|i| (i % 251) as u8).collect()] {
            for y in (0..HEIGHT).step_by(4) {
                let (op, data) = rectangle(&image, y, 4);
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
