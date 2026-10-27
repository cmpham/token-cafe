//! Pure framebuffer encoding and bounded update planning, independent of BLE.
use super::{FRAME_BYTES, HEIGHT, WIDTH};

pub fn encode_frame(pixels: &[u32]) -> Vec<u8> {
    assert_eq!(pixels.len(), WIDTH * HEIGHT);
    let mut out = vec![0; FRAME_BYTES];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let p = pixels[y * WIDTH + x];
            let color = ((((p >> 19) & 31) << 11) | (((p >> 10) & 63) << 5) | ((p >> 3) & 31)) as u16;
            let offset = (y * WIDTH + x) * 2;
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

/// Standard LZ4 block encoding with a bounded 64 KiB match window.
pub(super) fn lz4_block(data: &[u8]) -> Vec<u8> {
    fn extra(out: &mut Vec<u8>, mut n: usize) {
        while n >= 255 {
            out.push(255);
            n -= 255;
        }
        out.push(n as u8);
    }
    let mut table = vec![usize::MAX; 65536];
    let mut out = Vec::new();
    let mut anchor = 0;
    let mut i = 0;
    while i + 12 <= data.len() {
        let word = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
        let hash = (word.wrapping_mul(2654435761) >> 16) as usize;
        let previous = table[hash];
        table[hash] = i;
        if previous == usize::MAX || i - previous > 65535 || data[previous..previous + 4] != data[i..i + 4] {
            i += 1;
            continue;
        }
        let mut count = 4;
        while i + count < data.len() - 5 && data[previous + count] == data[i + count] {
            count += 1;
        }
        let literals = i - anchor;
        out.push(((literals.min(15) << 4) | (count - 4).min(15)) as u8);
        if literals >= 15 {
            extra(&mut out, literals - 15);
        }
        out.extend_from_slice(&data[anchor..i]);
        out.extend_from_slice(&((i - previous) as u16).to_le_bytes());
        if count - 4 >= 15 {
            extra(&mut out, count - 19);
        }
        i += count;
        anchor = i;
    }
    let literals = data.len() - anchor;
    out.push((literals.min(15) << 4) as u8);
    if literals >= 15 {
        extra(&mut out, literals - 15);
    }
    out.extend_from_slice(&data[anchor..]);
    out
}

/// Encode a full-width region; the planner bounds the encoded payload.
pub(super) fn rectangle(frame: &[u8], y: usize, height: usize) -> (u8, Vec<u8>) {
    let mut head = Vec::new();
    for n in [0, y as u16, WIDTH as u16, height as u16] {
        head.extend(n.to_le_bytes());
    }
    let pixels = &frame[y * WIDTH * 2..(y + height) * WIDTH * 2];
    let mut rle = head.clone();
    let mut i = 0;
    while i < pixels.len() {
        let color = &pixels[i..i + 2];
        let mut count = 1u16;
        while count < u16::MAX
            && i + (count as usize + 1) * 2 <= pixels.len()
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

/// Merge compressible rows into one request, but keep raw fallback within 4096 bytes.
pub(super) fn updates(frame: &[u8], baseline: Option<&[u8]>, fast: bool) -> Vec<(u8, Vec<u8>)> {
    let xor = baseline.map(|b| frame.iter().zip(b).map(|(a, b)| a ^ b).collect::<Vec<_>>());
    let encode = |y, height| {
        let (mut op, mut data) = rectangle(frame, y, height);
        if let Some(delta) = &xor {
            let (encoding, compressed) = rectangle(delta, y, height);
            if encoding == 6 && compressed.len() < data.len() {
                op = 8;
                data = compressed;
            }
        }
        if fast {
            for (encoding, source) in [(10, Some(frame)), (13, xor.as_deref())] {
                if let Some(source) = source {
                    let compressed = lz4_block(&source[y * WIDTH * 2..(y + height) * WIDTH * 2]);
                    if compressed.len() + 8 < data.len() {
                        data.truncate(8);
                        data.extend(compressed);
                        op = encoding;
                    }
                }
            }
        }
        if fast {
            op = match op {
                2 => 16,
                6 => 14,
                8 => 15,
                n => n,
            };
        }
        (op, data)
    };
    let mut result = Vec::new();
    let mut y = 0;
    while y < HEIGHT {
        let end = (y + 4).min(HEIGHT);
        let range = y * WIDTH * 2..end * WIDTH * 2;
        if baseline.is_some_and(|b| b[range.clone()] == frame[range]) {
            y = end;
            continue;
        }
        let mut height = end - y;
        let mut best = encode(y, height);
        // The payload cap bounds transfer time between touch polls. Sparse XOR
        // regions can span the full screen without dozens of empty-strip ACKs.
        while y + height < HEIGHT {
            let next_height = (height + 4).min(HEIGHT - y);
            let next = encode(y, next_height);
            if next.1.len() > 4096 {
                break;
            }
            best = next;
            height = next_height;
        }
        result.push(best);
        y += height;
    }
    result
}
