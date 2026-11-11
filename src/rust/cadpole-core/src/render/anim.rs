//! Animated images, written in-house: **APNG** on top of the PNG the
//! renderer already writes, and **GIF** with its own LZW.
//!
//! **APNG.** A PNG whose `IHDR` is followed by an `acTL` (frame count, play
//! count), whose first frame is the ordinary image — an `fcTL` before the
//! `IDAT`s, so a viewer that knows nothing of animation shows frame 0 — and
//! whose later frames are `fcTL` + `fdAT` pairs. Each frame after the first
//! carries only the **sub-rectangle that changed** against the frame before
//! it, and inside that rectangle every pixel that did not change is written
//! **transparent** (`dispose_op = NONE`, `blend_op = OVER`: an opaque pixel
//! replaces what is under it, a transparent one leaves it, and the result
//! stays), so a part turning on a still backdrop costs the pixels the part
//! swept and nothing else — the transparent runs deflate to almost nothing.
//! A frame whose source carries partial alpha falls back to `blend_op =
//! SOURCE` over its whole rectangle, since blending would then change what
//! is shown. The zlib stream of a rectangle is what the `png` crate would
//! write for it as an image of its own (its `IDAT` payloads, concatenated),
//! so the compressor is the one every still render uses and nothing new is
//! trusted with the bytes.
//!
//! **GIF.** One palette for the whole animation — 255 colours by **median
//! cut** over a sample of every frame's pixels, and index 255 reserved as
//! the transparent one — every frame **Floyd–Steinberg dithered** against
//! it, the changed sub-rectangle per frame with every unchanged pixel in it
//! written transparent (disposal "do not dispose", so the canvas keeps what
//! was there), a `NETSCAPE2.0` loop block, and LZW at an 8-bit minimum code
//! size with the code width growing from 9 to 12 bits and a clear when the
//! table fills. Changed and unchanged are decided on the *source* pixels,
//! not the dithered ones — error diffusion spreads a change rightwards and
//! down through pixels whose source did not move — and a changed pixel takes
//! the new frame's own full-frame dither, so the seam between kept and
//! rewritten pixels is two dithers of the same colours.
//!
//! Both encoders are deterministic: the same frames give the same bytes.

use std::collections::HashMap;
use std::io::Cursor;

use super::Image;

// ---------------------------------------------------------------------------
// PNG chunks
// ---------------------------------------------------------------------------

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (n, slot) in t.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    t
}

/// CRC-32 as PNG specifies it (ISO 3309), over the chunk type and data.
pub fn crc32(bytes: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// The chunks of a PNG, in order, each as (type, data). Checks the
/// signature and every CRC, so a reader can trust what it gets.
pub fn png_chunks(bytes: &[u8]) -> Result<Vec<([u8; 4], Vec<u8>)>, String> {
    if bytes.len() < 8 || bytes[..8] != PNG_SIGNATURE {
        return Err("not a PNG: bad signature".into());
    }
    let mut out = Vec::new();
    let mut i = 8;
    while i + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let kind: [u8; 4] = [bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]];
        let end = i + 8 + len;
        if end + 4 > bytes.len() {
            return Err(format!("chunk {} runs past the end", String::from_utf8_lossy(&kind)));
        }
        let crc = u32::from_be_bytes([bytes[end], bytes[end + 1], bytes[end + 2], bytes[end + 3]]);
        if crc != crc32(&bytes[i + 4..end]) {
            return Err(format!("chunk {} has a bad CRC", String::from_utf8_lossy(&kind)));
        }
        out.push((kind, bytes[i + 8..end].to_vec()));
        i = end + 4;
    }
    if i != bytes.len() {
        return Err("trailing bytes after the last chunk".into());
    }
    Ok(out)
}

/// The IHDR of an 8-bit RGBA image.
fn ihdr(width: u32, height: u32) -> [u8; 13] {
    let mut d = [0u8; 13];
    d[..4].copy_from_slice(&width.to_be_bytes());
    d[4..8].copy_from_slice(&height.to_be_bytes());
    d[8] = 8; // bit depth
    d[9] = 6; // colour type: RGBA
    d[10] = 0; // compression
    d[11] = 0; // filter
    d[12] = 0; // no interlace
    d
}

/// The zlib stream of an RGBA8 image's filtered scanlines: what the `png`
/// crate writes for it, its IDAT payloads joined. One frame's worth of
/// `fdAT` data, or the still image's `IDAT`.
fn compress_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::High);
        let mut w = enc.write_header().expect("png header");
        w.write_image_data(rgba).expect("png data");
    }
    let mut out = Vec::new();
    for (kind, data) in png_chunks(&png).expect("our own PNG parses") {
        if &kind == b"IDAT" {
            out.extend_from_slice(&data);
        }
    }
    out
}

/// Decode an RGBA8 PNG (one the renderer wrote, or one frame of
/// `format = "frames"`) back to an [`Image`].
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("png: no output size")?;
    let mut buf = vec![0u8; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let rgba = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => buf[..n * 4].to_vec(),
        (png::ColorType::Rgb, png::BitDepth::Eight) => {
            let mut out = Vec::with_capacity(n * 4);
            for p in buf[..n * 3].chunks_exact(3) {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
            out
        }
        (png::ColorType::Grayscale, png::BitDepth::Eight) => {
            let mut out = Vec::with_capacity(n * 4);
            for &g in &buf[..n] {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        (c, d) => return Err(format!("png: {c:?} at {d:?} bits is not a frame the animator writes")),
    };
    Ok(Image { width: w, height: h, rgba })
}

// ---------------------------------------------------------------------------
// Deltas
// ---------------------------------------------------------------------------

/// The smallest rectangle holding every pixel that differs between two
/// frames of the same size, as (x, y, w, h); `None` when nothing differs.
/// `stride` is bytes per pixel, and only the first `channels` of them are
/// compared (so a GIF can ignore alpha).
fn changed_rect(prev: &[u8], next: &[u8], width: u32, height: u32, stride: usize, channels: usize) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (width as usize, height as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..h {
        let row = y * w * stride;
        for x in 0..w {
            let i = row + x * stride;
            if prev[i..i + channels] != next[i..i + channels] {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    if x0 == usize::MAX {
        None
    } else {
        Some((x0 as u32, y0 as u32, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32))
    }
}

fn crop(src: &[u8], width: u32, stride: usize, rect: (u32, u32, u32, u32)) -> Vec<u8> {
    let (x, y, w, h) = rect;
    let mut out = Vec::with_capacity((w * h) as usize * stride);
    for row in y..y + h {
        let start = (row * width + x) as usize * stride;
        out.extend_from_slice(&src[start..start + w as usize * stride]);
    }
    out
}

// ---------------------------------------------------------------------------
// APNG
// ---------------------------------------------------------------------------

struct ApngFrame {
    rect: (u32, u32, u32, u32),
    /// The frame's delay as a fraction of a second, numerator over
    /// denominator, as `fcTL` carries it: `(1, 30)` is exactly 30 fps.
    delay: (u16, u16),
    /// `blend_op`: 1 (OVER) when the rectangle's unchanged pixels are
    /// transparent, 0 (SOURCE) when it is written whole.
    blend: u8,
    /// The zlib stream of the rectangle's scanlines.
    data: Vec<u8>,
}

/// An APNG under construction: push frames in order, then `finish`.
pub struct ApngEncoder {
    width: u32,
    height: u32,
    plays: u32,
    frames: Vec<ApngFrame>,
    prev: Option<Vec<u8>>,
}

impl ApngEncoder {
    /// `loop_forever` writes `num_plays = 0`; otherwise the animation plays
    /// once.
    pub fn new(width: u32, height: u32, loop_forever: bool) -> ApngEncoder {
        ApngEncoder { width, height, plays: if loop_forever { 0 } else { 1 }, frames: Vec::new(), prev: None }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Append a frame shown for `delay` seconds, a fraction `(numerator,
    /// denominator)` — `(1, 30)` for a 30 fps frame, exactly. The first
    /// frame is stored whole; every later one as the rectangle that changed
    /// against the frame before it, its unchanged pixels transparent (a
    /// frame identical to its predecessor stores one transparent pixel).
    pub fn push(&mut self, img: &Image, delay: (u16, u16)) -> Result<(), String> {
        if img.width != self.width || img.height != self.height {
            return Err(format!(
                "apng: frame {} is {}x{}, not the animation's {}x{}",
                self.frames.len(),
                img.width,
                img.height,
                self.width,
                self.height
            ));
        }
        if img.rgba.len() != (self.width * self.height * 4) as usize {
            return Err("apng: frame has the wrong number of bytes".into());
        }
        let (rect, blend, sub) = match &self.prev {
            None => ((0, 0, self.width, self.height), 0u8, img.rgba.clone()),
            Some(p) => {
                let rect = changed_rect(p, &img.rgba, self.width, self.height, 4, 4).unwrap_or((0, 0, 1, 1));
                let mut sub = crop(&img.rgba, self.width, 4, rect);
                // OVER needs every written pixel opaque and every kept pixel
                // transparent; a source with partial alpha is written whole.
                if sub.chunks_exact(4).all(|px| px[3] == 255) {
                    let prev_sub = crop(p, self.width, 4, rect);
                    for (dst, old) in sub.chunks_exact_mut(4).zip(prev_sub.chunks_exact(4)) {
                        if dst == old {
                            dst.copy_from_slice(&[0, 0, 0, 0]);
                        }
                    }
                    (rect, 1, sub)
                } else {
                    (rect, 0, sub)
                }
            }
        };
        let data = compress_rgba(rect.2, rect.3, &sub);
        let delay = (delay.0.max(1), delay.1.max(1));
        self.frames.push(ApngFrame { rect, delay, blend, data });
        self.prev = Some(img.rgba.clone());
        Ok(())
    }

    /// The finished file. With no frames, an empty vector.
    pub fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.frames.is_empty() {
            return out;
        }
        out.extend_from_slice(&PNG_SIGNATURE);
        push_chunk(&mut out, b"IHDR", &ihdr(self.width, self.height));
        let mut actl = Vec::new();
        actl.extend_from_slice(&(self.frames.len() as u32).to_be_bytes());
        actl.extend_from_slice(&self.plays.to_be_bytes());
        push_chunk(&mut out, b"acTL", &actl);
        let mut seq: u32 = 0;
        for (i, f) in self.frames.iter().enumerate() {
            let mut fctl = Vec::with_capacity(26);
            fctl.extend_from_slice(&seq.to_be_bytes());
            seq += 1;
            fctl.extend_from_slice(&f.rect.2.to_be_bytes());
            fctl.extend_from_slice(&f.rect.3.to_be_bytes());
            fctl.extend_from_slice(&f.rect.0.to_be_bytes());
            fctl.extend_from_slice(&f.rect.1.to_be_bytes());
            fctl.extend_from_slice(&f.delay.0.to_be_bytes());
            fctl.extend_from_slice(&f.delay.1.to_be_bytes());
            fctl.push(0); // dispose_op: NONE
            fctl.push(f.blend); // blend_op: SOURCE for a whole frame, OVER for a delta
            push_chunk(&mut out, b"fcTL", &fctl);
            if i == 0 {
                push_chunk(&mut out, b"IDAT", &f.data);
            } else {
                let mut fdat = Vec::with_capacity(4 + f.data.len());
                fdat.extend_from_slice(&seq.to_be_bytes());
                seq += 1;
                fdat.extend_from_slice(&f.data);
                push_chunk(&mut out, b"fdAT", &fdat);
            }
        }
        push_chunk(&mut out, b"IEND", &[]);
        out
    }
}

// ---------------------------------------------------------------------------
// GIF: the palette
// ---------------------------------------------------------------------------

/// Median cut: split the box with the widest channel range at its median
/// along that channel, repeatedly, until `count` boxes; each box's colour is
/// its mean. Deterministic in the sample order.
fn median_cut(samples: &[[u8; 3]], count: usize) -> Vec<[u8; 3]> {
    if samples.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut boxes: Vec<Vec<[u8; 3]>> = vec![samples.to_vec()];
    while boxes.len() < count {
        // The box with the widest range on any channel, ties to the first.
        let mut best: Option<(usize, usize, i32)> = None; // (box, channel, range)
        for (bi, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for c in 0..3 {
                let (mut lo, mut hi) = (255i32, 0i32);
                for p in b {
                    lo = lo.min(p[c] as i32);
                    hi = hi.max(p[c] as i32);
                }
                let range = hi - lo;
                if best.map(|(_, _, r)| range > r).unwrap_or(true) {
                    best = Some((bi, c, range));
                }
            }
        }
        let Some((bi, c, range)) = best else { break };
        if range == 0 {
            break;
        }
        let mut b = boxes.swap_remove(bi);
        b.sort_by_key(|p| (p[c], p[0], p[1], p[2]));
        let mid = b.len() / 2;
        let right = b.split_off(mid);
        boxes.push(b);
        boxes.push(right);
    }
    let mut palette: Vec<[u8; 3]> = boxes
        .iter()
        .map(|b| {
            let n = b.len() as u32;
            let sum = b.iter().fold([0u32; 3], |acc, p| [acc[0] + p[0] as u32, acc[1] + p[1] as u32, acc[2] + p[2] as u32]);
            [((sum[0] + n / 2) / n) as u8, ((sum[1] + n / 2) / n) as u8, ((sum[2] + n / 2) / n) as u8]
        })
        .collect();
    palette.sort();
    palette.dedup();
    palette
}

/// A palette with a nearest-colour lookup: exact for the first 32 k
/// distinct colours asked about, then whatever the cache holds (a lookup
/// per pixel is a walk over 256 entries; the cache keeps the dither
/// cheap).
struct Quantizer {
    palette: Vec<[u8; 3]>,
    cache: HashMap<u32, u8>,
}

impl Quantizer {
    fn new(palette: Vec<[u8; 3]>) -> Quantizer {
        Quantizer { palette, cache: HashMap::new() }
    }

    fn nearest(&mut self, r: i32, g: i32, b: i32) -> u8 {
        let (r, g, b) = (r.clamp(0, 255), g.clamp(0, 255), b.clamp(0, 255));
        let key = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        if let Some(&i) = self.cache.get(&key) {
            return i;
        }
        let mut best = 0usize;
        let mut best_d = i32::MAX;
        for (i, p) in self.palette.iter().enumerate() {
            let (dr, dg, db) = (p[0] as i32 - r, p[1] as i32 - g, p[2] as i32 - b);
            let d = dr * dr + dg * dg + db * db;
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        if self.cache.len() < 1 << 16 {
            self.cache.insert(key, best as u8);
        }
        best as u8
    }
}

/// Floyd–Steinberg: each pixel takes the nearest palette colour and passes
/// its error to the right (7/16), below-left (3/16), below (5/16) and
/// below-right (1/16).
fn dither(img: &Image, q: &mut Quantizer) -> Vec<u8> {
    let (w, h) = (img.width as usize, img.height as usize);
    let mut out = vec![0u8; w * h];
    // Errors for this row and the next, three channels each, in 1/16ths.
    let mut cur = vec![[0i32; 3]; w + 2];
    let mut next = vec![[0i32; 3]; w + 2];
    for y in 0..h {
        for e in next.iter_mut() {
            *e = [0; 3];
        }
        for x in 0..w {
            let i = (y * w + x) * 4;
            let src = [img.rgba[i] as i32, img.rgba[i + 1] as i32, img.rgba[i + 2] as i32];
            let want = [src[0] + cur[x + 1][0] / 16, src[1] + cur[x + 1][1] / 16, src[2] + cur[x + 1][2] / 16];
            let idx = q.nearest(want[0], want[1], want[2]);
            out[y * w + x] = idx;
            let p = q.palette[idx as usize];
            let err = [
                want[0].clamp(0, 255) - p[0] as i32,
                want[1].clamp(0, 255) - p[1] as i32,
                want[2].clamp(0, 255) - p[2] as i32,
            ];
            for c in 0..3 {
                cur[x + 2][c] += err[c] * 7;
                next[x][c] += err[c] * 3;
                next[x + 1][c] += err[c] * 5;
                next[x + 2][c] += err[c];
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    out
}

/// The palette and every frame's indices, before anything is written: what
/// the encoder commits to, and what the tests decode back to.
#[derive(Debug)]
pub struct GifPlan {
    pub width: u32,
    pub height: u32,
    /// At most 255 colours; index [`TRANSPARENT`] is the kept pixel.
    pub palette: Vec<[u8; 3]>,
    /// Per frame, the rectangle written and its indices, row by row: the
    /// dithered colour where the source changed, [`TRANSPARENT`] where it
    /// did not (never in the first frame, which is written whole).
    pub frames: Vec<((u32, u32, u32, u32), Vec<u8>)>,
}

/// The palette index a GIF frame writes where the pixel did not change.
pub const TRANSPARENT: u8 = 255;

/// Quantise `count` frames, fetched by `frame(i)` (each an RGBA image of
/// the same size), to one palette: a sample of at most about a million
/// pixels over all the frames, median-cut to 255 colours, then every frame
/// dithered and its changed rectangle found on the source pixels, the
/// unchanged pixels inside it transparent.
pub fn plan_gif(width: u32, height: u32, count: usize, frame: &mut dyn FnMut(usize) -> Image) -> Result<GifPlan, String> {
    if count == 0 {
        return Err("gif: no frames".into());
    }
    let px = (width as usize) * (height as usize);
    let total = px * count;
    let stride = (total / (1 << 20)).max(1);
    // Pass 1: the sample. An odd step so a stride that divides the width
    // does not sample one column.
    let step = if stride == 1 { 1 } else { next_odd(stride) };
    let mut samples: Vec<[u8; 3]> = Vec::with_capacity(total / step + count);
    let mut phase = 0usize;
    for i in 0..count {
        let img = frame(i);
        check_size(&img, width, height, i)?;
        let mut j = phase;
        while j < px {
            let k = j * 4;
            samples.push([img.rgba[k], img.rgba[k + 1], img.rgba[k + 2]]);
            j += step;
        }
        phase = (phase + 1) % step;
    }
    let palette = median_cut(&samples, TRANSPARENT as usize);
    let mut q = Quantizer::new(palette.clone());
    // Pass 2: dither and delta.
    let mut frames = Vec::with_capacity(count);
    let mut prev: Option<Vec<u8>> = None;
    for i in 0..count {
        let img = frame(i);
        check_size(&img, width, height, i)?;
        let indices = dither(&img, &mut q);
        let (rect, written) = match &prev {
            None => ((0, 0, width, height), indices),
            Some(p) => {
                let rect = changed_rect(p, &img.rgba, width, height, 4, 3).unwrap_or((0, 0, 1, 1));
                let mut written = crop(&indices, width, 1, rect);
                let (x, y, w, h) = rect;
                for row in 0..h as usize {
                    for col in 0..w as usize {
                        let k = ((y as usize + row) * width as usize + x as usize + col) * 4;
                        if p[k..k + 3] == img.rgba[k..k + 3] {
                            written[row * w as usize + col] = TRANSPARENT;
                        }
                    }
                }
                (rect, written)
            }
        };
        frames.push((rect, written));
        prev = Some(img.rgba);
    }
    Ok(GifPlan { width, height, palette, frames })
}

fn next_odd(n: usize) -> usize {
    if n % 2 == 0 { n + 1 } else { n }
}

fn check_size(img: &Image, width: u32, height: u32, i: usize) -> Result<(), String> {
    if img.width != width || img.height != height || img.rgba.len() != (width * height * 4) as usize {
        return Err(format!("gif: frame {i} is {}x{}, not the animation's {width}x{height}", img.width, img.height));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// GIF: the file
// ---------------------------------------------------------------------------

/// LZW as GIF uses it: variable-width codes, LSB first, a clear code at the
/// start and whenever the table is full, packed into 255-byte sub-blocks.
struct Lzw {
    min_code_size: u8,
    out: Vec<u8>,
    block: Vec<u8>,
    acc: u64,
    nbits: u32,
}

impl Lzw {
    fn new(min_code_size: u8) -> Lzw {
        Lzw { min_code_size, out: Vec::new(), block: Vec::with_capacity(255), acc: 0, nbits: 0 }
    }

    fn emit(&mut self, code: u16, width: u32) {
        self.acc |= (code as u64) << self.nbits;
        self.nbits += width;
        while self.nbits >= 8 {
            self.block.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.nbits -= 8;
            if self.block.len() == 255 {
                self.flush_block();
            }
        }
    }

    fn flush_block(&mut self) {
        if !self.block.is_empty() {
            self.out.push(self.block.len() as u8);
            self.out.extend_from_slice(&self.block);
            self.block.clear();
        }
    }

    fn encode(mut self, indices: &[u8]) -> Vec<u8> {
        let clear: u16 = 1 << self.min_code_size;
        let eoi: u16 = clear + 1;
        let mut width: u32 = self.min_code_size as u32 + 1;
        let mut next: u16 = eoi + 1;
        let mut table: HashMap<u32, u16> = HashMap::new();
        self.emit(clear, width);
        let mut it = indices.iter();
        let Some(&first) = it.next() else {
            self.emit(eoi, width);
            if self.nbits > 0 {
                self.block.push((self.acc & 0xFF) as u8);
            }
            self.flush_block();
            self.out.push(0);
            return self.out;
        };
        let mut prefix: u16 = first as u16;
        for &k in it {
            let key = ((prefix as u32) << 8) | k as u32;
            if let Some(&code) = table.get(&key) {
                prefix = code;
                continue;
            }
            self.emit(prefix, width);
            if next < 4096 {
                table.insert(key, next);
                next += 1;
                if next > (1 << width) && width < 12 {
                    width += 1;
                }
            } else {
                self.emit(clear, width);
                table.clear();
                width = self.min_code_size as u32 + 1;
                next = eoi + 1;
            }
            prefix = k as u16;
        }
        self.emit(prefix, width);
        self.emit(eoi, width);
        if self.nbits > 0 {
            self.block.push((self.acc & 0xFF) as u8);
            self.acc = 0;
            self.nbits = 0;
        }
        self.flush_block();
        self.out.push(0);
        self.out
    }
}

/// Write a planned GIF: `delay_cs(i)` is the hundredths of a second frame
/// `i` is shown for; `loop_forever` writes the NETSCAPE2.0 block with a
/// count of 0.
pub fn write_gif(plan: &GifPlan, delay_cs: &dyn Fn(usize) -> u16, loop_forever: bool) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&(plan.width as u16).to_le_bytes());
    out.extend_from_slice(&(plan.height as u16).to_le_bytes());
    // Global colour table of 256 entries, 8 bits per primary, not sorted.
    out.push(0xF7);
    out.push(0); // background colour index
    out.push(0); // pixel aspect ratio: square
    for i in 0..256 {
        let p = plan.palette.get(i).copied().unwrap_or([0, 0, 0]);
        out.extend_from_slice(&p);
    }
    if loop_forever {
        out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
        out.extend_from_slice(b"NETSCAPE2.0");
        out.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);
    }
    for (i, (rect, written)) in plan.frames.iter().enumerate() {
        // Graphic control: disposal 1 (do not dispose); from the second
        // frame on, index 255 is transparent, so a kept pixel stays.
        let packed = if i == 0 { 0x04 } else { 0x05 };
        out.extend_from_slice(&[0x21, 0xF9, 0x04, packed]);
        out.extend_from_slice(&delay_cs(i).to_le_bytes());
        out.extend_from_slice(&[TRANSPARENT, 0x00]);
        let (x, y, w, h) = *rect;
        out.push(0x2C);
        out.extend_from_slice(&(x as u16).to_le_bytes());
        out.extend_from_slice(&(y as u16).to_le_bytes());
        out.extend_from_slice(&(w as u16).to_le_bytes());
        out.extend_from_slice(&(h as u16).to_le_bytes());
        out.push(0); // no local colour table, not interlaced
        out.push(8);
        out.extend_from_slice(&Lzw::new(8).encode(written));
    }
    out.push(0x3B);
    out
}

/// Plan and write in one call.
pub fn encode_gif(
    width: u32,
    height: u32,
    count: usize,
    frame: &mut dyn FnMut(usize) -> Image,
    delay_cs: &dyn Fn(usize) -> u16,
    loop_forever: bool,
) -> Result<Vec<u8>, String> {
    let plan = plan_gif(width, height, count, frame)?;
    Ok(write_gif(&plan, delay_cs, loop_forever))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scene that moves: a dark backdrop with a horizontal gradient, and
    /// a bright square that walks across it frame by frame.
    fn frames(n: usize, w: u32, h: u32) -> Vec<Image> {
        (0..n)
            .map(|i| {
                let mut img = Image { width: w, height: h, rgba: vec![0; (w * h * 4) as usize] };
                for y in 0..h {
                    for x in 0..w {
                        let k = ((y * w + x) * 4) as usize;
                        img.rgba[k] = (x * 255 / w.max(1)) as u8 / 3;
                        img.rgba[k + 1] = 30;
                        img.rgba[k + 2] = (y * 255 / h.max(1)) as u8 / 3;
                        img.rgba[k + 3] = 255;
                    }
                }
                // Twelve pixels a step for an eight-pixel square, so the
                // backdrop between its old place and its new one is inside
                // the delta rectangle and unchanged.
                let sx = (i as u32 * 12) % (w - 8);
                for y in 5..13.min(h) {
                    for x in sx..sx + 8 {
                        img.put(x, y, [250, 200, 90, 255]);
                    }
                }
                img
            })
            .collect()
    }

    // ---- an APNG reader, enough to check what the encoder wrote ----

    struct Fctl {
        seq: u32,
        w: u32,
        h: u32,
        x: u32,
        y: u32,
        delay: (u16, u16),
        dispose: u8,
        blend: u8,
    }

    fn parse_fctl(d: &[u8]) -> Fctl {
        let u = |i: usize| u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
        Fctl {
            seq: u(0),
            w: u(4),
            h: u(8),
            x: u(12),
            y: u(16),
            delay: (u16::from_be_bytes([d[20], d[21]]), u16::from_be_bytes([d[22], d[23]])),
            dispose: d[24],
            blend: d[25],
        }
    }

    /// Decode one frame's zlib stream as a standalone PNG of the rectangle.
    fn decode_sub(w: u32, h: u32, zlib: &[u8]) -> Image {
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIGNATURE);
        push_chunk(&mut png, b"IHDR", &ihdr(w, h));
        push_chunk(&mut png, b"IDAT", zlib);
        push_chunk(&mut png, b"IEND", &[]);
        decode_png(&png).unwrap()
    }

    /// Every frame of an APNG, composited (dispose NONE, blend SOURCE).
    fn apng_frames(bytes: &[u8]) -> (u32, u32, Vec<Fctl>, Vec<Image>) {
        let chunks = png_chunks(bytes).unwrap();
        assert_eq!(&chunks[0].0, b"IHDR");
        let d = &chunks[0].1;
        let w = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
        let h = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
        assert_eq!(&chunks[1].0, b"acTL");
        let a = &chunks[1].1;
        let num_frames = u32::from_be_bytes([a[0], a[1], a[2], a[3]]);
        let plays = u32::from_be_bytes([a[4], a[5], a[6], a[7]]);
        let mut canvas: Option<Image> = None;
        let mut fctls = Vec::new();
        let mut out = Vec::new();
        let mut pending: Option<(Fctl, Vec<u8>)> = None;
        let mut expect_seq = 0u32;
        for (kind, data) in &chunks[2..] {
            match kind {
                b"fcTL" => {
                    if let Some((f, z)) = pending.take() {
                        out.push(composite(&mut canvas, w, h, &f, &z));
                        fctls.push(f);
                    }
                    let f = parse_fctl(data);
                    assert_eq!(f.seq, expect_seq, "fcTL sequence");
                    expect_seq += 1;
                    pending = Some((f, Vec::new()));
                }
                b"IDAT" => pending.as_mut().unwrap().1.extend_from_slice(data),
                b"fdAT" => {
                    let seq = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    assert_eq!(seq, expect_seq, "fdAT sequence");
                    expect_seq += 1;
                    pending.as_mut().unwrap().1.extend_from_slice(&data[4..]);
                }
                b"IEND" => {}
                other => panic!("unexpected chunk {}", String::from_utf8_lossy(other)),
            }
        }
        if let Some((f, z)) = pending.take() {
            out.push(composite(&mut canvas, w, h, &f, &z));
            fctls.push(f);
        }
        assert_eq!(out.len() as u32, num_frames, "acTL frame count");
        (plays, w, fctls, out)
    }

    fn composite(canvas: &mut Option<Image>, w: u32, h: u32, f: &Fctl, zlib: &[u8]) -> Image {
        assert_eq!(f.dispose, 0);
        let sub = decode_sub(f.w, f.h, zlib);
        let first = canvas.is_none();
        let c = canvas.get_or_insert_with(|| Image { width: w, height: h, rgba: vec![0; (w * h * 4) as usize] });
        if f.blend == 0 {
            assert!(first, "SOURCE is the whole first frame; deltas are OVER");
            c.blit(&sub, f.x, f.y);
        } else {
            // OVER with the alphas the encoder writes: 255 replaces, 0 keeps.
            for y in 0..sub.height {
                for x in 0..sub.width {
                    let i = ((y * sub.width + x) * 4) as usize;
                    match sub.rgba[i + 3] {
                        255 => c.put(f.x + x, f.y + y, [sub.rgba[i], sub.rgba[i + 1], sub.rgba[i + 2], 255]),
                        0 => {}
                        a => panic!("a delta pixel with alpha {a}"),
                    }
                }
            }
        }
        c.clone()
    }

    /// The APNG's chunks parse back with their CRCs, the counts and the
    /// sequence numbers are right, the deltas are the moving square's
    /// rectangle and never the whole frame, an unchanged frame is one
    /// pixel, and compositing every frame gives the source frames exactly.
    #[test]
    fn apng_round_trips_with_deltas() {
        let (w, h) = (40, 24);
        let mut src = frames(6, w, h);
        src.insert(3, src[2].clone()); // a held frame
        let mut enc = ApngEncoder::new(w, h, true);
        for f in &src {
            enc.push(f, (1, 30)).unwrap();
        }
        assert_eq!(enc.frame_count(), 7);
        let bytes = enc.finish();
        // The still image is a PNG a plain decoder reads as frame 0.
        let still = decode_png(&bytes).unwrap();
        assert_eq!(still.rgba, src[0].rgba);
        let (plays, width, fctls, decoded) = apng_frames(&bytes);
        assert_eq!((plays, width), (0, w));
        assert_eq!(decoded.len(), 7);
        for (i, (d, s)) in decoded.iter().zip(&src).enumerate() {
            assert_eq!(d.rgba, s.rgba, "frame {i}");
        }
        assert_eq!((fctls[0].x, fctls[0].y, fctls[0].w, fctls[0].h), (0, 0, w, h), "the first frame is whole");
        assert_eq!((fctls[0].blend, fctls[1].blend), (0, 1), "the first frame is SOURCE, a delta is OVER");
        assert_eq!(fctls[0].delay, (1, 30), "a 30 fps frame is exactly 1/30 s");
        // Frame 1 changed where the square was and where it went: x 0..20,
        // rows 5..13.
        assert_eq!((fctls[1].x, fctls[1].y, fctls[1].w, fctls[1].h), (0, 5, 20, 8));
        assert_eq!((fctls[3].w, fctls[3].h), (1, 1), "a held frame stores one pixel");
        assert!(fctls[2].w < w && fctls[2].h < h, "a delta is never the whole frame");
        // Inside a delta only the pixels that changed are opaque: the
        // square's old and new places, not the backdrop between them.
        let chunks = png_chunks(&bytes).unwrap();
        let fdat1 = chunks.iter().filter(|(k, _)| k == b"fdAT").next().unwrap();
        let sub1 = decode_sub(fctls[1].w, fctls[1].h, &fdat1.1[4..]);
        let opaque = sub1.rgba.chunks_exact(4).filter(|p| p[3] == 255).count();
        let transparent = sub1.rgba.chunks_exact(4).filter(|p| p[3] == 0).count();
        assert_eq!(opaque + transparent, (fctls[1].w * fctls[1].h) as usize);
        assert!(opaque > 0 && transparent > 0, "{opaque} opaque, {transparent} transparent");
        for (k, px) in sub1.rgba.chunks_exact(4).enumerate() {
            let (x, y) = (fctls[1].x + (k as u32 % fctls[1].w), fctls[1].y + (k as u32 / fctls[1].w));
            let i = ((y * w + x) * 4) as usize;
            let changed = src[0].rgba[i..i + 4] != src[1].rgba[i..i + 4];
            assert_eq!(px[3] == 255, changed, "pixel ({x}, {y})");
        }
        // Deterministic.
        let mut enc2 = ApngEncoder::new(w, h, true);
        for f in &src {
            enc2.push(f, (1, 30)).unwrap();
        }
        assert_eq!(enc2.finish(), bytes);
        // A wrong-sized frame is refused; playing once writes plays = 1.
        let mut enc3 = ApngEncoder::new(w, h, false);
        assert!(enc3.push(&Image::new(3, 3, [0.0; 3]), (1, 100)).unwrap_err().contains("3x3"));
        enc3.push(&src[0], (0, 0)).unwrap();
        let (plays, _, _, _) = apng_frames(&enc3.finish());
        assert_eq!(plays, 1);
        assert!(ApngEncoder::new(w, h, true).finish().is_empty());
    }

    // ---- a GIF reader ----

    fn lzw_decode(min_code_size: u8, data: &[u8], expect: usize) -> Vec<u8> {
        let clear = 1u16 << min_code_size;
        let eoi = clear + 1;
        let mut width = min_code_size as u32 + 1;
        let mut dict: Vec<Vec<u8>> = (0..clear).map(|i| vec![i as u8]).collect();
        dict.push(Vec::new());
        dict.push(Vec::new());
        let mut out = Vec::with_capacity(expect);
        let mut acc: u64 = 0;
        let mut nbits = 0u32;
        let mut prev: Option<Vec<u8>> = None;
        let mut bytes = data.iter();
        let mut done = false;
        while !done {
            while nbits < width {
                match bytes.next() {
                    Some(&b) => {
                        acc |= (b as u64) << nbits;
                        nbits += 8;
                    }
                    None => panic!("lzw: ran out of data"),
                }
            }
            let code = (acc & ((1 << width) - 1)) as u16;
            acc >>= width;
            nbits -= width;
            if code == clear {
                dict.truncate(eoi as usize + 1);
                width = min_code_size as u32 + 1;
                prev = None;
                continue;
            }
            if code == eoi {
                done = true;
                continue;
            }
            let entry = if (code as usize) < dict.len() {
                dict[code as usize].clone()
            } else if let Some(p) = &prev {
                let mut e = p.clone();
                e.push(p[0]);
                e
            } else {
                panic!("lzw: bad code {code}");
            };
            out.extend_from_slice(&entry);
            if let Some(p) = prev {
                let mut n = p.clone();
                n.push(entry[0]);
                if dict.len() < 4096 {
                    dict.push(n);
                }
                if dict.len() == (1 << width) && width < 12 {
                    width += 1;
                }
            }
            prev = Some(entry);
        }
        out
    }

    struct GifFrame {
        rect: (u16, u16, u16, u16),
        delay_cs: u16,
        indices: Vec<u8>,
    }

    fn gif_frames(bytes: &[u8]) -> (u16, u16, Vec<[u8; 3]>, bool, Vec<GifFrame>) {
        assert_eq!(&bytes[..6], b"GIF89a");
        let w = u16::from_le_bytes([bytes[6], bytes[7]]);
        let h = u16::from_le_bytes([bytes[8], bytes[9]]);
        assert_eq!(bytes[10], 0xF7, "a 256-entry global table");
        let mut i = 13;
        let palette: Vec<[u8; 3]> = (0..256).map(|k| [bytes[i + 3 * k], bytes[i + 3 * k + 1], bytes[i + 3 * k + 2]]).collect();
        i += 768;
        let mut looped = false;
        let mut frames = Vec::new();
        let mut delay = 0u16;
        loop {
            match bytes[i] {
                0x21 => {
                    let label = bytes[i + 1];
                    i += 2;
                    if label == 0xFF {
                        assert_eq!(bytes[i], 0x0B);
                        assert_eq!(&bytes[i + 1..i + 12], b"NETSCAPE2.0");
                        i += 12;
                        assert_eq!(&bytes[i..i + 5], &[0x03, 0x01, 0x00, 0x00, 0x00]);
                        i += 5;
                        looped = true;
                    } else if label == 0xF9 {
                        assert_eq!(bytes[i], 4);
                        let packed = bytes[i + 1];
                        assert_eq!(packed & 0x1C, 0x04, "disposal: do not dispose");
                        let transparent = packed & 1 == 1;
                        assert_eq!(transparent, !frames.is_empty(), "the first frame has no transparency, every later one has");
                        assert_eq!(bytes[i + 4], TRANSPARENT);
                        delay = u16::from_le_bytes([bytes[i + 2], bytes[i + 3]]);
                        i += 6;
                    } else {
                        panic!("unexpected extension {label:#x}");
                    }
                }
                0x2C => {
                    let x = u16::from_le_bytes([bytes[i + 1], bytes[i + 2]]);
                    let y = u16::from_le_bytes([bytes[i + 3], bytes[i + 4]]);
                    let fw = u16::from_le_bytes([bytes[i + 5], bytes[i + 6]]);
                    let fh = u16::from_le_bytes([bytes[i + 7], bytes[i + 8]]);
                    assert_eq!(bytes[i + 9], 0);
                    let min = bytes[i + 10];
                    i += 11;
                    let mut data = Vec::new();
                    loop {
                        let n = bytes[i] as usize;
                        i += 1;
                        if n == 0 {
                            break;
                        }
                        data.extend_from_slice(&bytes[i..i + n]);
                        i += n;
                    }
                    let indices = lzw_decode(min, &data, fw as usize * fh as usize);
                    assert_eq!(indices.len(), fw as usize * fh as usize, "a frame decodes to its own pixel count");
                    frames.push(GifFrame { rect: (x, y, fw, fh), delay_cs: delay, indices });
                }
                0x3B => {
                    assert_eq!(i + 1, bytes.len(), "the trailer is last");
                    break;
                }
                other => panic!("unexpected block {other:#x} at {i}"),
            }
        }
        (w, h, palette, looped, frames)
    }

    /// The GIF decodes: the header, the loop block, the palette, every
    /// frame's LZW back to exactly the indices the plan committed to, the
    /// deltas as sub-rectangles composited onto the canvas, and the whole
    /// file is deterministic.
    #[test]
    fn gif_round_trips_to_the_planned_indices() {
        let (w, h) = (48, 20);
        let mut src = frames(5, w, h);
        src.insert(2, src[1].clone());
        let plan = plan_gif(w, h, src.len(), &mut |i| src[i].clone()).unwrap();
        assert!(plan.palette.len() <= 255 && plan.palette.len() >= 16, "{} colours", plan.palette.len());
        assert_eq!(plan.frames[0].0, (0, 0, w, h));
        assert!(!plan.frames[0].1.contains(&TRANSPARENT), "the first frame is written whole");
        assert_eq!(plan.frames[2].0, (0, 0, 1, 1), "a held frame is one pixel");
        assert_eq!(plan.frames[2].1, vec![TRANSPARENT], "...and that pixel is kept");
        assert!(plan.frames[1].0 .2 < w, "a delta is a sub-rectangle");
        let bytes = write_gif(&plan, &|i| 3 + i as u16, true);
        let (gw, gh, palette, looped, decoded) = gif_frames(&bytes);
        assert_eq!((gw as u32, gh as u32, looped), (w, h, true));
        assert_eq!(&palette[..plan.palette.len()], &plan.palette[..]);
        assert_eq!(decoded.len(), src.len());
        let mut canvas = vec![0u8; (w * h) as usize];
        for (i, f) in decoded.iter().enumerate() {
            assert_eq!(f.delay_cs, 3 + i as u16);
            let (x, y, fw, fh) = f.rect;
            assert_eq!((x as u32, y as u32, fw as u32, fh as u32), plan.frames[i].0);
            // The LZW decodes to exactly the planned indices; a transparent
            // index keeps the canvas, and is written only where the source
            // did not change; outside the rectangle nothing moved.
            assert_eq!(f.indices, plan.frames[i].1, "frame {i}");
            for row in 0..fh as usize {
                for col in 0..fw as usize {
                    let k = (y as usize + row) * w as usize + x as usize + col;
                    let ix = f.indices[row * fw as usize + col];
                    if i > 0 {
                        let unchanged = src[i].rgba[k * 4..k * 4 + 3] == src[i - 1].rgba[k * 4..k * 4 + 3];
                        assert_eq!(ix == TRANSPARENT, unchanged, "frame {i} at {k}");
                    }
                    if ix != TRANSPARENT {
                        canvas[k] = ix;
                    }
                }
            }
            if i > 0 {
                for py in 0..h as usize {
                    for px in 0..w as usize {
                        let inside = px >= x as usize && px < (x + fw) as usize && py >= y as usize && py < (y + fh) as usize;
                        if !inside {
                            let k = (py * w as usize + px) * 4;
                            assert_eq!(&src[i].rgba[k..k + 3], &src[i - 1].rgba[k..k + 3], "frame {i}: a pixel outside the delta moved");
                        }
                    }
                }
            }
            // What is displayed is close to the source: dithering keeps the
            // mean of a flat region, so the average error is small even
            // though one pixel may be a palette step away.
            let mut err = 0i64;
            for (k, &ix) in canvas.iter().enumerate() {
                let p = plan.palette[ix as usize];
                for c in 0..3 {
                    err += (src[i].rgba[k * 4 + c] as i64 - p[c] as i64).abs();
                }
            }
            assert!(err / (w * h * 3) as i64 <= 4, "frame {i}: mean error {}", err / (w * h * 3) as i64);
        }
        // Deterministic, and the one-call form is the same bytes.
        let again = encode_gif(w, h, src.len(), &mut |i| src[i].clone(), &|i| 3 + i as u16, true).unwrap();
        assert_eq!(again, bytes);
        // Not looping writes no NETSCAPE block.
        let once = write_gif(&plan, &|_| 4, false);
        assert!(!gif_frames(&once).3);
        assert!(encode_gif(w, h, 0, &mut |_| src[0].clone(), &|_| 4, true).unwrap_err().contains("no frames"));
        let bad = plan_gif(w, h, 1, &mut |_| Image::new(2, 2, [0.0; 3])).unwrap_err();
        assert!(bad.contains("2x2"), "{bad}");
    }

    /// LZW over data that fills the table several times: a 4096-code
    /// table on 100 k pixels of noise-ish indices, decoded back exactly.
    #[test]
    fn lzw_survives_table_resets() {
        let indices: Vec<u8> = (0..100_000u32).map(|i| ((i * 7919) ^ (i >> 3)) as u8).collect();
        let data = Lzw::new(8).encode(&indices);
        let mut joined = Vec::new();
        let mut i = 0;
        loop {
            let n = data[i] as usize;
            i += 1;
            if n == 0 {
                break;
            }
            joined.extend_from_slice(&data[i..i + n]);
            i += n;
        }
        assert_eq!(i, data.len());
        assert_eq!(lzw_decode(8, &joined, indices.len()), indices);
        // A run of one colour compresses hard; an empty run is a clear and an
        // end.
        let flat = vec![7u8; 5000];
        let d = Lzw::new(8).encode(&flat);
        assert!(d.len() < 200, "{}", d.len());
        let e = Lzw::new(8).encode(&[]);
        assert_eq!(e.last(), Some(&0));
    }

    /// Median cut on a two-colour image is those two colours; the nearest
    /// lookup is exact and cached.
    #[test]
    fn median_cut_and_nearest() {
        let mut samples = vec![[10u8, 20, 30]; 50];
        samples.extend(std::iter::repeat_n([200u8, 210, 220], 50));
        let p = median_cut(&samples, 255);
        assert_eq!(p, vec![[10, 20, 30], [200, 210, 220]]);
        let mut q = Quantizer::new(p);
        assert_eq!(q.nearest(12, 22, 28), 0);
        assert_eq!(q.nearest(190, 255, 255), 1);
        assert_eq!(q.nearest(-5, 300, 0), 0);
        assert_eq!(q.cache.len(), 3);
        assert_eq!(median_cut(&[], 4), vec![[0, 0, 0]]);
        // A gradient splits into distinct colours in sorted order.
        let g: Vec<[u8; 3]> = (0..=255u8).map(|v| [v, v, v]).collect();
        let p = median_cut(&g, 8);
        assert_eq!(p.len(), 8);
        assert!(p.windows(2).all(|w| w[0] < w[1]));
    }

    /// The chunk reader refuses a broken CRC and a bad signature.
    #[test]
    fn png_chunks_check_crcs() {
        let img = Image::new(4, 3, [0.2, 0.3, 0.4]);
        let mut bytes = img.to_png();
        let chunks = png_chunks(&bytes).unwrap();
        assert_eq!(&chunks[0].0, b"IHDR");
        assert_eq!(&chunks.last().unwrap().0, b"IEND");
        let n = bytes.len();
        bytes[n - 6] ^= 1;
        assert!(png_chunks(&bytes).unwrap_err().contains("bad CRC"));
        assert!(png_chunks(b"hello").unwrap_err().contains("signature"));
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        let back = decode_png(&img.to_png()).unwrap();
        assert_eq!(back.rgba, img.rgba);
    }
}
