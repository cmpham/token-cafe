//! BLE v2 platform adapter. Rendering and protocol semantics live in shared crates.
use super::{Error, SERVICE, read_key, state::Store};
use crate::{approval::Broker, scene::Scene};
use btleplug::{
    api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification, WriteType},
    platform::{Manager, Peripheral},
};
use companion_core::{
    authority::{Dispatch, Peer},
    crypto::{self, Keys},
    sync,
    transport::Transport,
    wire::{envelope::Payload, *},
};
use futures::{Stream, StreamExt};
use std::{
    collections::VecDeque,
    io::Read,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::time::{sleep, timeout};
use uuid::Uuid;
const RX: Uuid = Uuid::from_u128(0x7e57cafe_0001_4000_8000_000000000005);
const TX: Uuid = Uuid::from_u128(0x7e57cafe_0001_4000_8000_000000000006);
const AUTH: Uuid = Uuid::from_u128(0x7e57cafe_0001_4000_8000_000000000007);
fn err<T>(r: Result<T, crypto::Error>) -> Result<T, Error> {
    r.map_err(|e| format!("Protocol v2: {e:?}").into())
}
#[derive(Clone)]
struct Board {
    device: String,
    name: String,
    key: [u8; 32],
    production: bool,
}
fn registry() -> Result<Vec<Board>, Error> {
    let path = std::env::var_os("TC_BLE_REGISTRY").map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".token-cafe/boards.json")
    });
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let entries =
        json["boards"].as_array().filter(|a| !a.is_empty() && a.len() <= 4).ok_or("Register one to four boards")?;
    let mut result = vec![];
    for b in entries {
        let device = b["device_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 64 && s.is_ascii())
            .ok_or("Invalid registered board ID")?;
        if result.iter().any(|r: &Board| r.device == device) {
            return Err("Duplicate registered board".into());
        }
        let production = match b["trust"].as_str() {
            Some("production") => true,
            Some("qa") => false,
            _ => return Err("Missing board trust class".into()),
        };
        result.push(Board {
            device: device.into(),
            name: b["name"].as_str().ok_or("Missing board name")?.into(),
            key: read_key(b["key_path"].as_str().ok_or("Missing board key")?)?,
            production,
        });
    }
    Ok(result)
}
pub struct SemanticMirror {
    shared: Arc<Mutex<Store>>,
    status: Arc<Mutex<Vec<String>>>,
}
impl SemanticMirror {
    pub fn start() -> Self {
        let mut epoch = [0; 16];
        std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut epoch)).expect("OS entropy");
        let shared = Arc::new(Mutex::new(Store::new(epoch)));
        let status = Arc::new(Mutex::new(vec!["starting v2".into()]));
        let mirror = Self { shared: shared.clone(), status: status.clone() };
        std::thread::spawn(move || {
            let boards = match registry() {
                Ok(b) => b,
                Err(e) => {
                    *status.lock().unwrap() = vec![format!("v2 pairing required: {e}")];
                    return;
                }
            };
            *status.lock().unwrap() = boards.iter().map(|b| format!("{} connecting", b.name)).collect();
            let runtime = tokio::runtime::Runtime::new().expect("BLE runtime");
            runtime.block_on(async move {
                let mut tasks = vec![];
                for (i, board) in boards.into_iter().enumerate() {
                    let shared = shared.clone();
                    let status = status.clone();
                    tasks.push(tokio::spawn(async move {
                        loop {
                            let text = match run(&board, shared.clone(), status.clone(), i).await {
                                Ok(()) => "disconnected".into(),
                                Err(e) => {
                                    eprintln!("BLE v2 {}: {e}", board.name);
                                    format!("disconnected: {e}")
                                }
                            };
                            status.lock().unwrap()[i] = format!("{} {text}", board.name);
                            sleep(Duration::from_secs(3)).await;
                        }
                    }));
                }
                for task in tasks {
                    let _ = task.await;
                }
            });
        });
        mirror
    }
    pub fn synchronize(&mut self, scene: &mut Scene, broker: Option<&Broker>) -> Vec<String> {
        let mut s = self.shared.lock().unwrap();
        s.ingest(scene, broker);
        s.navigation.drain(..).collect()
    }
    pub fn status(&self) -> String {
        self.status.lock().unwrap().join("; ")
    }
}
struct Link {
    peripheral: Peripheral,
    rx: Characteristic,
    notifications: Pin<Box<dyn Stream<Item = ValueNotification> + Send>>,
    transport: Transport,
    epoch: Vec<u8>,
    device: String,
}
// CoreBluetooth/btleplug can combine a cached peripheral name with the
// current advertisement as "old [advertised]". This is only a discovery hint;
// the exact device ID and pinned-key proof are verified after connecting.
fn advertised_name_matches(observed: &str, expected: &str) -> bool {
    observed == expected
        || observed
            .strip_suffix(']')
            .and_then(|s| s.rsplit_once(" ["))
            .is_some_and(|(_, advertised)| advertised == expected)
}
async fn connect(board: &Board, start: Instant) -> Result<Link, Error> {
    let manager = Manager::new().await?;
    let adapter = manager.adapters().await?.into_iter().next().ok_or("No BLE adapter")?;
    adapter.start_scan(ScanFilter { services: vec![SERVICE] }).await?;
    sleep(Duration::from_secs(5)).await;
    let mut candidates = vec![];
    for p in adapter.peripherals().await? {
        if p.properties().await?.is_some_and(|v| {
            v.services.contains(&SERVICE)
                && v.local_name.as_deref().is_some_and(|name| advertised_name_matches(name, &board.name))
        }) {
            candidates.push(p);
        }
    }
    // Scan is shared by registered links; leaving it active avoids cancelling another link's discovery.
    if candidates.len() != 1 {
        return Err("Registered board absent or ambiguous".into());
    }
    let peripheral = candidates.remove(0);
    peripheral.connect().await?;
    let setup = async {
        peripheral.discover_services().await?;
        let chars = peripheral.characteristics();
        let auth = chars.iter().find(|c| c.uuid == AUTH).ok_or("Board needs v2 firmware")?;
        let idle = peripheral.read(auth).await?;
        if idle.len() < 2 || idle[0] != 0 || idle[1] as usize != idle.len() - 2 || idle[2..] != *board.device.as_bytes()
        {
            return Err::<Link, Error>("Registered board identity mismatch".into());
        }
        let mut hn = [0; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut hn)?;
        peripheral.write(auth, &[&[1], hn.as_slice()].concat(), WriteType::WithResponse).await?;
        let proof = peripheral.read(auth).await?;
        if proof.len() != 33 || proof[0] != 2 {
            return Err("Invalid board proof".into());
        }
        let t = err(crypto::transcript(&board.device, &hn, proof[1..17].try_into().unwrap()))?;
        err(crypto::verify_auth(&board.key, b"board", &t, &proof[17..]))?;
        let tx = chars.iter().find(|c| c.uuid == TX).ok_or("Missing v2 TX")?;
        let notifications = peripheral.notifications().await?;
        peripheral.subscribe(tx).await?;
        peripheral
            .write(
                auth,
                &[&[3], crypto::auth_tag(&board.key, b"host", &t).as_slice()].concat(),
                WriteType::WithResponse,
            )
            .await?;
        let caps = peripheral.read(auth).await?;
        if caps.len() != 4 || caps[..2] != [4, 0] {
            return Err("Invalid authenticated capacity".into());
        }
        let cap = u16::from_le_bytes([caps[2], caps[3]]) as usize;
        let keys = Keys::derive(&board.key, &t);
        let transport = err(Transport::new(&keys, true, cap, start.elapsed().as_millis() as u64))?;
        let rx = chars.iter().find(|c| c.uuid == RX).ok_or("Missing v2 RX")?.clone();
        Ok(Link {
            peripheral: peripheral.clone(),
            rx,
            notifications,
            transport,
            epoch: keys.epoch.to_vec(),
            device: board.device.clone(),
        })
    }
    .await;
    if setup.is_err() {
        let _ = peripheral.disconnect().await;
    }
    setup
}
async fn run(
    board: &Board,
    shared: Arc<Mutex<Store>>,
    status: Arc<Mutex<Vec<String>>>,
    index: usize,
) -> Result<(), Error> {
    let start = Instant::now();
    let mut link = timeout(Duration::from_secs(30), connect(board, start)).await??;
    let result = serve(board, &mut link, shared, status, index, start).await;
    let _ = timeout(Duration::from_secs(2), link.peripheral.disconnect()).await;
    result
}
async fn serve(
    board: &Board,
    link: &mut Link,
    shared: Arc<Mutex<Store>>,
    status: Arc<Mutex<Vec<String>>>,
    index: usize,
    start: Instant,
) -> Result<(), Error> {
    let authority = shared.lock().unwrap().epoch.to_vec();
    let env = |p| sync::envelope(&link.epoch, &authority, p);
    let mut peer = Peer::new(link.device.clone(), link.epoch.clone(), board.production, true);
    let mut hello = false;
    let mut resync = false;
    let mut last_sent = None;
    let mut outstanding = VecDeque::new();
    let mut pending: VecDeque<(Action, u64)> = VecDeque::new();
    let mut bulk = VecDeque::new();
    let mut blob_queue = VecDeque::new();
    let mut blob_turn = false;
    let mut navigation: Vec<(Action, std::sync::mpsc::Receiver<bool>, u64)> = vec![];
    let mut heartbeat = Instant::now();
    loop {
        let now = start.elapsed().as_millis() as u64;
        err(link.transport.check(now))?;
        if hello && heartbeat.elapsed() >= Duration::from_secs(1) {
            err(link.transport.enqueue(&env(Payload::Heartbeat(Heartbeat {
                applied_revision: shared.lock().unwrap().authority.model.revision,
                monotonic_ms: now,
            }))))?;
            heartbeat = Instant::now();
        }
        // Complete provider delivery asynchronously; socket delivery is not execution.
        let mut completed = vec![];
        {
            let mut store = shared.lock().unwrap();
            for (a, at) in &pending {
                let outcome = store.broker.as_ref().and_then(|b| b.take_outcome(&a.request_id));
                if let Some(ok) = outcome.or_else(|| (now - *at > 3000).then_some(false)) {
                    let r = store.authority.finish(&mut peer, a, ok);
                    store.keep_terminal(&a.session_id);
                    completed.push((a.id, r));
                }
            }
        }
        for (id, r) in completed {
            pending.retain(|(a, _)| a.id != id);
            err(link.transport.enqueue(&env(Payload::ActionResult(r))))?;
        }
        let mut nav_done = vec![];
        for (a, rx, at) in &navigation {
            if let Some(ok) = rx.try_recv().ok().or_else(|| (now - *at > 3000).then_some(false)) {
                let mut s = shared.lock().unwrap();
                let r = s.authority.finish(&mut peer, a, ok);
                err(link.transport.enqueue(&env(Payload::ActionResult(r))))?;
                nav_done.push(a.id);
            }
        }
        navigation.retain(|(a, _, _)| !nav_done.contains(&a.id));
        if hello && (resync || last_sent.is_some()) && bulk.is_empty() && outstanding.len() < 8 {
            let model = shared.lock().unwrap().authority.model.clone();
            if resync
                || last_sent.as_ref().is_none_or(|old: &companion_core::model::Model| old.revision != model.revision)
            {
                let messages = match last_sent.as_ref().filter(|_| !resync) {
                    Some(old) => err(sync::update(old, &model, &link.epoch, &authority))?,
                    None => err(sync::snapshot(&model, &link.epoch, &authority))?,
                };
                bulk.extend(messages);
                outstanding.push_back(model.revision);
                last_sent = Some(model);
                resync = false;
            }
        }
        if link.transport.queued() == 0 {
            let e = if blob_turn && !blob_queue.is_empty() {
                blob_queue.pop_front()
            } else {
                bulk.pop_front().or_else(|| blob_queue.pop_front())
            };
            blob_turn = !blob_turn;
            if let Some(e) = e {
                err(link.transport.enqueue(&e))?;
            }
        }
        // Receive independently of outgoing bulk. WithResponse bounds the platform's TX queue.
        tokio::select! {
          value=link.notifications.next()=>{
            let n=value.ok_or("BLE notifications ended")?;if n.uuid!=TX{continue;}
            if let Some((e,_))=err(link.transport.receive(&n.value,now))?{
                if e.connection_epoch!=link.epoch{return Err("Wrong connection epoch".into());}
                if let Some(Payload::Hello(h))=e.payload.as_ref(){
                    if hello||h.device_id!=board.device||h.minor!=0||h.asset_digest!=cafe_ui::ASSET_DIGEST||h.max_envelope_bytes!=4096||h.max_blob_bytes!=32768||!h.required_capabilities.iter().all(|c|[1,2,3,4,5,6].contains(c))||![1,2,6].iter().all(|c|h.capabilities.contains(c)){return Err("Unsupported v2 firmware/assets/capabilities".into());}
                    if h.input_injection_enabled&&board.production{return Err("Production key cannot authenticate an injectable QA image".into());}
                    // QA keys cannot decide against real adapters, irrespective of Hello claims.
                    let capabilities=if board.production{vec![1,2,3,4,5,6]}else{vec![1,2,5,6]};
                    err(link.transport.enqueue(&env(Payload::HelloAck(HelloAck{minor:0,asset_digest:cafe_ui::ASSET_DIGEST.to_vec(),capabilities,required_capabilities:vec![1,2,6],heartbeat_ms:1000,session_limit:64,blob_limit:32768,max_envelope_bytes:4096}))))?;hello=true;continue;
                }
                if !hello||e.authority_epoch!=authority{return Err("Wrong authority epoch".into());}
                match e.payload.ok_or("Empty message")?{
                    Payload::Resync(_)=>{resync=true;outstanding.clear();},
                    Payload::Applied(a)=>{status.lock().unwrap()[index]=format!("{} synced v2",board.name);if last_sent.as_ref().is_none_or(|m|a.revision>m.revision){return Err("Future Applied revision".into());}while outstanding.front().is_some_and(|r|*r<=a.revision){outstanding.pop_front();}},
                    Payload::Heartbeat(_)=>(),
                    Payload::BlobGet(b)=>{if !blob_queue.is_empty(){return Err("Concurrent blob request".into());}let body=shared.lock().unwrap().body(&b.sha256).map(Vec::from);if let Some(body)=body{for chunk in err(sync::chunks(&body))?{blob_queue.push_back(env(Payload::BlobChunk(chunk)));}}else{err(link.transport.enqueue(&env(Payload::BlobUnavailable(BlobUnavailable{sha256:b.sha256,reason:2}))))?;}},
                    Payload::BlobCancel(b)=>blob_queue.retain(|e|!matches!(&e.payload,Some(Payload::BlobChunk(c)) if c.sha256==b.sha256)),
                    Payload::Action(a)=>{
                        let mut s=shared.lock().unwrap();let body=s.body(&a.blob_id).map(Vec::from);let (mut result,dispatch)=err(s.authority.action(&mut peer,&a,now,body.as_deref()))?;
                        match dispatch{
                            Some(Dispatch::Decision{session,request,decision})=>{s.keep_terminal(&session);if s.decide(&request,decision){pending.push_back((a.clone(),now));}else{result=s.authority.finish(&mut peer,&a,false);s.keep_terminal(&session);}},
                            Some(Dispatch::Desktop(id))=>{if let Some(url)=s.targets.get(&id).cloned(){if navigation.len()<16{let(tx,rx)=std::sync::mpsc::channel();std::thread::spawn(move||{#[cfg(target_os="macos")]let ok=std::process::Command::new("/usr/bin/open").arg(url).status().is_ok_and(|s|s.success());#[cfg(not(target_os="macos"))]let ok={let _=url;false};let _=tx.send(ok);});navigation.push((a.clone(),rx,now));}else{result=s.authority.finish(&mut peer,&a,false);}}else{result=s.authority.finish(&mut peer,&a,false);}},None=>(),
                        }
                        err(link.transport.enqueue(&env(Payload::ActionResult(result))))?;
                    },_=>return Err("Unexpected board message".into()),
                }
            }
          },
          _=sleep(Duration::from_millis(1))=>{
            if let Some(packet)=err(link.transport.next_fragment())?{timeout(Duration::from_secs(2),link.peripheral.write(&link.rx,&packet,WriteType::WithResponse)).await??;}
          }
        }
    }
}

/// Isolated hardware QA host. It accepts QA-class keys only and has no provider
/// broker or desktop opener. Every side effect is a synthetic recorded outcome.
pub async fn exercise_fixture(
    model: companion_core::model::Model,
    bodies: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
    duration: Duration,
    log_path: &std::path::Path,
) -> Result<(), Error> {
    use std::io::Write;
    let boards = registry()?;
    if boards.len() != 1 || boards[0].production {
        return Err("Fixture host requires exactly one QA-class board".into());
    }
    let start = Instant::now();
    let mut link = timeout(Duration::from_secs(30), connect(&boards[0], start)).await??;
    let mut authority = companion_core::authority::Authority { model };
    let epoch = [0x51; 16];
    let mut peer = Peer::new(boards[0].device.clone(), link.epoch.clone(), true, true);
    let mut log = std::fs::File::create(log_path)?;
    let mut last_sent = None;
    let mut queue = VecDeque::new();
    let mut heartbeat = Instant::now();
    let mut ready = false;
    while start.elapsed() < duration {
        let now = start.elapsed().as_millis() as u64;
        err(link.transport.check(now))?;
        if ready && heartbeat.elapsed() >= Duration::from_secs(1) {
            err(link.transport.enqueue(&sync::envelope(
                &link.epoch,
                &epoch,
                Payload::Heartbeat(Heartbeat { applied_revision: authority.model.revision, monotonic_ms: now }),
            )))?;
            heartbeat = Instant::now();
        }
        if link.transport.queued() == 0
            && let Some(e) = queue.pop_front()
        {
            err(link.transport.enqueue(&e))?;
        }
        tokio::select! {
            n=link.notifications.next()=>{
                let n=n.ok_or("Disconnected QA board")?;if n.uuid!=TX{continue;}
                if let Some((e,_))=err(link.transport.receive(&n.value,now))?{
                    if e.connection_epoch!=link.epoch{return Err("QA epoch mismatch".into());}
                    let env=|p|sync::envelope(&link.epoch,&epoch,p);
                    match e.payload.ok_or("empty")?{
                        Payload::Hello(h)=>{if h.asset_digest!=cafe_ui::ASSET_DIGEST||!h.input_injection_enabled{return Err("QA firmware/assets mismatch".into());}err(link.transport.enqueue(&env(Payload::HelloAck(HelloAck{asset_digest:cafe_ui::ASSET_DIGEST.to_vec(),capabilities:vec![1,2,3,4,5,6],required_capabilities:vec![1,2,6],heartbeat_ms:1000,session_limit:64,blob_limit:32768,max_envelope_bytes:4096,..Default::default()}))))?;ready=true;},
                        Payload::Resync(_)=>{queue.extend(err(sync::snapshot(&authority.model,&link.epoch,&epoch))?);last_sent=Some(authority.model.clone());},
                        Payload::BlobGet(b)=>{if let Some(body)=bodies.get(&b.sha256){for c in err(sync::chunks(body))?{queue.push_back(env(Payload::BlobChunk(c)));}}},
                        Payload::Action(a)=>{
                            let before=authority.model.clone();let (mut result,dispatch)=err(authority.action(&mut peer,&a,now,bodies.get(&a.blob_id).map(Vec::as_slice)))?;
                            if dispatch.is_some(){result=authority.finish(&mut peer,&a,true);}
                            writeln!(log,"{}",serde_json::json!({"ms":now,"id":a.id,"kind":a.kind,"session":a.session_id,"request":a.request_id,"decision":a.decision,"status":result.status,"reason":result.reason,"revision":authority.model.revision,"selected":authority.model.view.selected_session_id,"page":authority.model.view.page}))?;log.flush()?;
                            if authority.model!=before{queue.extend(err(sync::update(last_sent.as_ref().unwrap(),&authority.model,&link.epoch,&epoch))?);last_sent=Some(authority.model.clone());}
                            err(link.transport.enqueue(&env(Payload::ActionResult(result))))?;
                        },
                        Payload::Applied(a)=>{writeln!(log,"{}",serde_json::json!({"ms":now,"applied":a.revision}))?;log.flush()?;},
                        Payload::Heartbeat(_)=>(),_=>return Err("unexpected fixture message".into()),
                    }
                }
            },
            _=sleep(Duration::from_millis(1))=>{if let Some(packet)=err(link.transport.next_fragment())?{timeout(Duration::from_secs(2),link.peripheral.write(&link.rx,&packet,WriteType::WithResponse)).await??;}}
        }
    }
    link.peripheral.disconnect().await?;
    Ok(())
}

#[cfg(test)]
mod discovery_tests {
    use super::advertised_name_matches;
    #[test]
    fn cached_macos_name_preserves_exact_advertisement_match() {
        assert!(advertised_name_matches("TokenCafe-QA", "TokenCafe-QA"));
        assert!(advertised_name_matches("Old [TokenCafe-QA]", "TokenCafe-QA"));
        assert!(!advertised_name_matches("Old [TokenCafe-QA-other]", "TokenCafe-QA"));
        assert!(!advertised_name_matches("TokenCafe-QA [Other]", "TokenCafe-QA"));
    }
}
