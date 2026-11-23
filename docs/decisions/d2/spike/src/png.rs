//! A minimal PNG writer: 1-bit greyscale, uncompressed deflate blocks. Enough
//! to look at point patterns without another dependency.

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    b << 16 | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend(data);
    out.extend(&body);
    out.extend(crc32(&body).to_be_bytes());
}

/// Writes `pixels` (row-major, `true` = black) as a 1-bit PNG.
pub fn write(path: &std::path::Path, w: usize, h: usize, pixels: &[bool]) -> std::io::Result<()> {
    // Rows: a filter byte, then 8 pixels per byte, 1 = white.
    let stride = w.div_ceil(8);
    let mut raw = Vec::with_capacity(h * (stride + 1));
    for y in 0..h {
        raw.push(0);
        for bx in 0..stride {
            let mut byte = 0u8;
            for bit in 0..8 {
                let x = bx * 8 + bit;
                let white = x >= w || !pixels[y * w + x];
                byte |= (white as u8) << (7 - bit);
            }
            raw.push(byte);
        }
    }
    // zlib stream of stored blocks.
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend(*b);
    }
    z.extend(adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([1, 0, 0, 0, 0]); // 1-bit greyscale
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out)
}
