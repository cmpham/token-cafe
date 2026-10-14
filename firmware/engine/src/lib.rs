//! The same semantic UI core runs on QEMU and the physical board.
#![no_std]
extern crate alloc;
use alloc::{boxed::Box, string::String, vec, vec::Vec};
use companion_core::{
    client::{Client, Receipt},
    crypto::{self, Keys},
    transport::Transport,
};
struct Alloc;
unsafe extern "C" {
    fn cafe_alloc(size: usize, align: usize) -> *mut u8;
    fn cafe_free(ptr: *mut u8);
    fn cafe_random(ptr: *mut u8, len: usize);
    fn cafe_persist(ptr: *const u8, len: usize) -> bool;
    fn abort() -> !;
}
unsafe impl core::alloc::GlobalAlloc for Alloc {
    unsafe fn alloc(&self, l: core::alloc::Layout) -> *mut u8 {
        unsafe { cafe_alloc(l.size(), l.align()) }
    }
    unsafe fn dealloc(&self, p: *mut u8, _: core::alloc::Layout) {
        unsafe { cafe_free(p) }
    }
}
#[global_allocator]
static ALLOC: Alloc = Alloc;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { abort() }
}
pub struct Engine {
    device: String,
    key: [u8; 32],
    cap: usize,
    injection: bool,
    transcript: Vec<u8>,
    response: Vec<u8>,
    phase: u8,
    client: Option<Client>,
    link: Option<Transport>,
    frame: Vec<u32>,
    receipts: Vec<Receipt>,
}
impl Engine {
    fn fail(&mut self) {
        if let Some(c) = &mut self.client {
            c.disconnect();
        }
        self.link = None;
        self.phase = 0;
        self.response.clear();
        self.transcript.clear();
    }
    fn pump(&mut self) -> bool {
        if let (Some(c), Some(t)) = (&mut self.client, &mut self.link) {
            while t.queued() < 12 {
                let Some(e) = c.outgoing() else { break };
                if t.enqueue(&e).is_err() {
                    return false;
                }
            }
        }
        true
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_new(
    device: *const u8,
    len: usize,
    key: *const u8,
    capacity: usize,
    injection: bool,
) -> *mut Engine {
    if len == 0 || len > 64 || !(182..=509).contains(&capacity) {
        return core::ptr::null_mut();
    }
    let d = unsafe { core::slice::from_raw_parts(device, len) };
    let Ok(device) = core::str::from_utf8(d) else { return core::ptr::null_mut() };
    if !device.is_ascii() {
        return core::ptr::null_mut();
    }
    let key = unsafe { core::slice::from_raw_parts(key, 32) }.try_into().unwrap();
    Box::into_raw(Box::new(Engine {
        device: device.into(),
        key,
        cap: capacity,
        injection,
        transcript: vec![],
        response: vec![],
        phase: 0,
        client: None,
        link: None,
        frame: vec![0; 480 * 272],
        receipts: vec![],
    }))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_drop(e: *mut Engine) {
    if !e.is_null() {
        drop(unsafe { Box::from_raw(e) });
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_disconnect(e: *mut Engine) {
    unsafe { &mut *e }.fail();
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_auth_read(e: *mut Engine, out: *mut u8, capacity: usize) -> usize {
    let e = unsafe { &mut *e };
    let data = match e.phase {
        0 => {
            let mut b = vec![0, e.device.len() as u8];
            b.extend_from_slice(e.device.as_bytes());
            b
        }
        1 => e.response.clone(),
        2 => {
            let mut b = vec![4, 0];
            b.extend_from_slice(&(e.cap as u16).to_le_bytes());
            b
        }
        _ => return 0,
    };
    if capacity < data.len() {
        return 0;
    }
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), out, data.len()) };
    data.len()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_auth_write(e: *mut Engine, data: *const u8, len: usize, now: u64) -> bool {
    let e = unsafe { &mut *e };
    let data = unsafe { core::slice::from_raw_parts(data, len) };
    if data.len() == 17 && data[0] == 1 && e.phase == 0 {
        let mut bn = [0; 16];
        unsafe { cafe_random(bn.as_mut_ptr(), 16) };
        let Ok(t) = crypto::transcript(&e.device, data[1..].try_into().unwrap(), &bn) else { return false };
        e.response = vec![2];
        e.response.extend_from_slice(&bn);
        e.response.extend_from_slice(&crypto::auth_tag(&e.key, b"board", &t));
        e.transcript = t;
        e.phase = 1;
        return true;
    }
    if data.len() == 17
        && data[0] == 3
        && e.phase == 1
        && crypto::verify_auth(&e.key, b"host", &e.transcript, &data[1..]).is_ok()
    {
        let keys = Keys::derive(&e.key, &e.transcript);
        let mut c = Client::new(e.device.clone(), keys.epoch.to_vec(), e.injection, now);
        c.receipts = e.receipts.clone();
        e.client = Some(c);
        e.link = Transport::new(&keys, false, e.cap, now).ok();
        e.phase = 2;
        return e.pump();
    }
    e.fail();
    false
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_receive(e: *mut Engine, data: *const u8, len: usize, now: u64) -> bool {
    let e = unsafe { &mut *e };
    let data = unsafe { core::slice::from_raw_parts(data, len) };
    let ok = if let (Some(t), Some(c)) = (&mut e.link, &mut e.client) {
        match t.receive(data, now) {
            Ok(Some((env, raw))) => c.receive(env, &raw, now).is_ok(),
            Ok(None) => true,
            Err(_) => false,
        }
    } else {
        false
    };
    if !ok || !e.pump() {
        e.fail();
        return false;
    }
    true
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_next(e: *mut Engine, out: *mut u8, capacity: usize) -> usize {
    let e = unsafe { &mut *e };
    if capacity < e.cap || !e.pump() {
        return 0;
    }
    if let Some(t) = &mut e.link {
        if let Ok(Some(p)) = t.next_fragment() {
            unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), out, p.len()) };
            return p.len();
        }
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_tick(e: *mut Engine, now: u64) -> bool {
    let e = unsafe { &mut *e };
    let ok = if let (Some(t), Some(c)) = (&mut e.link, &mut e.client) {
        {
            let ok = t.check(now).is_ok() && c.tick(now).is_ok();
            c.reconcile_receipts(&mut persist);
            e.receipts = c.receipts.clone();
            ok
        }
    } else {
        true
    };
    if !ok || !e.pump() {
        e.fail();
        return false;
    }
    true
}
fn persist(receipts: &[Receipt]) -> bool {
    // Versioned bounded journal. NVS storage is responsible for atomic commit.
    let mut out = vec![2, receipts.len() as u8];
    for r in receipts {
        out.extend_from_slice(&r.authority);
        out.push(r.decision as u8);
        for value in [&r.session, &r.request] {
            out.extend_from_slice(&(value.len() as u16).to_le_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        out.extend_from_slice(&r.digest);
    }
    unsafe { cafe_persist(out.as_ptr(), out.len()) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_touch(e: *mut Engine, pressed: u8, x: i32, y: i32, now: u64) -> bool {
    let e = unsafe { &mut *e };
    let ok = if let Some(c) = &mut e.client {
        let ok = c.touch(pressed, x, y, now, &mut persist).is_ok();
        e.receipts = c.receipts.clone();
        ok
    } else {
        true
    };
    if !ok || !e.pump() {
        e.fail();
        return false;
    }
    true
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_draw(e: *mut Engine, now: u64) -> bool {
    let e = unsafe { &mut *e };
    let Some(c) = &mut e.client else { return false };
    c.render(&mut e.frame, now);
    true
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_pack(e: *mut Engine, out: *mut u16) {
    let e = unsafe { &mut *e };
    let dest = unsafe { core::slice::from_raw_parts_mut(out, 480 * 272) };
    for (p, c) in dest.iter_mut().zip(&e.frame) {
        *p = (((c >> 19) & 31) << 11 | ((c >> 10) & 63) << 5 | ((c >> 3) & 31)) as u16;
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_render(e: *mut Engine, out: *mut u16, now: u64) -> bool {
    if !unsafe { cafe_engine_draw(e, now) } {
        return false;
    }
    unsafe { cafe_engine_pack(e, out) };
    true
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_feedback(e: *mut Engine, out: *mut u16) -> bool {
    let e = unsafe { &mut *e };
    let Some(c) = &e.client else { return false };
    let dest = unsafe { core::slice::from_raw_parts_mut(out, 480 * 272) };
    c.feedback(|x, y| dest[y as usize * 480 + x as usize] = 0xfecb)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_presented(e: *mut Engine, now: u64) {
    if let Some(c) = &mut unsafe { &mut *e }.client {
        c.presented(now);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_status(e: *mut Engine) -> u32 {
    let e = unsafe { &mut *e };
    e.client.as_ref().map_or(0, |c| {
        u32::from(c.connected)
            | (u32::from(c.replica.ready) << 1)
            | (u32::from(c.sleeping) << 2)
            | ((c.view().page as u32) << 8)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cafe_engine_journal(e: *mut Engine, data: *const u8, len: usize) -> bool {
    let e = unsafe { &mut *e };
    let data = unsafe { core::slice::from_raw_parts(data, len) };
    let decode = || -> Option<Vec<Receipt>> {
        if len < 2 || data[0] != 2 || data[1] > 8 {
            return None;
        }
        let mut p = 2;
        let mut result = vec![];
        for _ in 0..data[1] {
            let authority = data.get(p..p + 16)?.to_vec();
            p += 16;
            let decision = *data.get(p)? as i32;
            p += 1;
            if !(1..=2).contains(&decision) {
                return None;
            }
            let mut strings = vec![];
            for _ in 0..2 {
                let size = u16::from_le_bytes(data.get(p..p + 2)?.try_into().ok()?) as usize;
                p += 2;
                if size == 0 || size > 128 {
                    return None;
                }
                strings.push(core::str::from_utf8(data.get(p..p + size)?).ok()?.into());
                p += size;
            }
            let digest = data.get(p..p + 32)?.to_vec();
            p += 32;
            result.push(Receipt {
                authority,
                session: strings.remove(0),
                request: strings.remove(0),
                digest,
                decision,
            });
        }
        if p != len {
            return None;
        }
        Some(result)
    };
    if let Some(r) = decode() {
        e.receipts = r;
        true
    } else {
        false
    }
}
