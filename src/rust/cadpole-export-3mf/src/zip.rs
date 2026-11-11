//! A tiny, deterministic ZIP writer and reader — the OPC container a 3MF
//! package lives in.
//!
//! Written here rather than pulled in because the requirements are narrow and
//! the format is not: we need **stored and deflated entries, no encryption, no
//! zip64, and byte-identical output for identical input**. Every entry's
//! timestamp is pinned to 1980-01-01 00:00:00 (the earliest an MS-DOS
//! timestamp can express), so two exports of the same model compare equal and
//! a golden file is meaningful. DEFLATE itself comes from `miniz_oxide`.
//!
//! The reader parses the **central directory**, the way a real zip reader
//! does, rather than walking local headers: that is what proves the offsets,
//! sizes and CRCs the writer put in the directory are right.

use std::collections::HashMap;

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;
/// 1980-01-01: year 0 in bits 9..15, month 1 in bits 5..8, day 1 in bits 0..4.
const DOS_DATE: u16 = 0x0021;
const DOS_TIME: u16 = 0;
const METHOD_STORE: u16 = 0;
const METHOD_DEFLATE: u16 = 8;
/// miniz_oxide level: 6 is the usual "good enough, still quick" setting.
const DEFLATE_LEVEL: u8 = 6;

/// Builds a zip archive in memory.
#[derive(Default)]
pub struct ZipWriter {
    out: Vec<u8>,
    entries: Vec<Central>,
}

struct Central {
    name: String,
    crc: u32,
    method: u16,
    compressed: u32,
    uncompressed: u32,
    offset: u32,
}

impl ZipWriter {
    pub fn new() -> ZipWriter {
        ZipWriter::default()
    }

    /// Add one file. Deflated unless deflating makes it bigger (tiny parts),
    /// in which case it is stored — which is also a valid zip entry and one
    /// fewer thing for a reader to get wrong.
    pub fn add(&mut self, name: &str, data: &[u8]) {
        let deflated = miniz_oxide::deflate::compress_to_vec(data, DEFLATE_LEVEL);
        let (method, body) = if deflated.len() < data.len() {
            (METHOD_DEFLATE, deflated)
        } else {
            (METHOD_STORE, data.to_vec())
        };
        let crc = crc32(data);
        let offset = self.out.len() as u32;
        let name_bytes = name.as_bytes();

        u32le(&mut self.out, LOCAL_SIG);
        u16le(&mut self.out, 20); // version needed: 2.0, deflate
        u16le(&mut self.out, 0); // flags
        u16le(&mut self.out, method);
        u16le(&mut self.out, DOS_TIME);
        u16le(&mut self.out, DOS_DATE);
        u32le(&mut self.out, crc);
        u32le(&mut self.out, body.len() as u32);
        u32le(&mut self.out, data.len() as u32);
        u16le(&mut self.out, name_bytes.len() as u16);
        u16le(&mut self.out, 0); // extra
        self.out.extend_from_slice(name_bytes);
        self.out.extend_from_slice(&body);

        self.entries.push(Central {
            name: name.to_owned(),
            crc,
            method,
            compressed: body.len() as u32,
            uncompressed: data.len() as u32,
            offset,
        });
    }

    /// The finished archive.
    pub fn finish(mut self) -> Vec<u8> {
        let cd_offset = self.out.len() as u32;
        for e in &self.entries {
            let name = e.name.as_bytes();
            u32le(&mut self.out, CENTRAL_SIG);
            u16le(&mut self.out, 20); // version made by: 2.0, MS-DOS
            u16le(&mut self.out, 20); // version needed
            u16le(&mut self.out, 0); // flags
            u16le(&mut self.out, e.method);
            u16le(&mut self.out, DOS_TIME);
            u16le(&mut self.out, DOS_DATE);
            u32le(&mut self.out, e.crc);
            u32le(&mut self.out, e.compressed);
            u32le(&mut self.out, e.uncompressed);
            u16le(&mut self.out, name.len() as u16);
            u16le(&mut self.out, 0); // extra
            u16le(&mut self.out, 0); // comment
            u16le(&mut self.out, 0); // disk number
            u16le(&mut self.out, 0); // internal attrs
            u32le(&mut self.out, 0); // external attrs
            u32le(&mut self.out, e.offset);
            self.out.extend_from_slice(name);
        }
        let cd_size = self.out.len() as u32 - cd_offset;
        let n = self.entries.len() as u16;
        u32le(&mut self.out, EOCD_SIG);
        u16le(&mut self.out, 0);
        u16le(&mut self.out, 0);
        u16le(&mut self.out, n);
        u16le(&mut self.out, n);
        u32le(&mut self.out, cd_size);
        u32le(&mut self.out, cd_offset);
        u16le(&mut self.out, 0); // archive comment
        self.out
    }
}

/// An archive read back: names in directory order, plus a lookup.
#[derive(Debug)]
pub struct Archive {
    pub names: Vec<String>,
    files: HashMap<String, Vec<u8>>,
}

impl Archive {
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.get(name).map(|v| v.as_slice())
    }
    pub fn text(&self, name: &str) -> Option<String> {
        self.get(name).map(|b| String::from_utf8_lossy(b).into_owned())
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Read an archive by its central directory, checking every CRC.
pub fn read(bytes: &[u8]) -> Result<Archive, String> {
    read_with(bytes, &Caps::UNLIMITED)
}

/// What a reader accepts of an archive it did not write (docs/security.md,
/// "Imports"): an imported 3MF is untrusted bytes, and a zip's central
/// directory is a list of promises about sizes that inflating checks only
/// after the memory is spent. Every cap here is checked against the
/// directory *before* an entry is inflated, and the inflater is then bounded
/// by the size the directory declared, so a directory that lies cannot buy
/// more memory than it admitted to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Entries in the archive, at most.
    pub max_entries: usize,
    /// Sum of the declared uncompressed sizes, at most, in bytes.
    pub max_inflated: u64,
    /// Declared uncompressed / compressed size of any one entry, at most.
    /// A deflated stream of zeros inflates about 1000:1, which is what a
    /// zip bomb is; a model file is text and compresses 5–20:1.
    pub max_ratio: u64,
}

impl Caps {
    /// No limits: what the writer's own tests read back with.
    pub const UNLIMITED: Caps = Caps { max_entries: usize::MAX, max_inflated: u64::MAX, max_ratio: u64::MAX };
}

/// [`read`] under `caps`. Refuses, naming the entry, an archive with more
/// entries than allowed, an entry whose name leaves the package (`..`, a
/// leading `/` or `\`, a NUL), a total or per-entry size over the caps, or
/// an entry that inflates to other than its declared size.
pub fn read_with(bytes: &[u8], caps: &Caps) -> Result<Archive, String> {
    read_some(bytes, caps, &|_| true)
}

/// [`read_with`], inflating only the entries `want` names (a saved
/// Bambu project read as a template is its settings and its root model, not
/// its meshes and pictures). Every entry's name is still checked; the size
/// caps count the entries read.
pub fn read_some(bytes: &[u8], caps: &Caps, want: &dyn Fn(&str) -> bool) -> Result<Archive, String> {
    // The end-of-central-directory record is the last thing in the file, at
    // most 65535 + 22 bytes from the end because of the archive comment.
    let start = bytes.len().saturating_sub(22 + 0xffff);
    let eocd = (start..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| u32at(bytes, i) == Some(EOCD_SIG))
        .ok_or("not a zip archive: no end-of-central-directory record")?;
    let count = u16at(bytes, eocd + 10).ok_or("truncated eocd")? as usize;
    let cd_offset = u32at(bytes, eocd + 16).ok_or("truncated eocd")? as usize;
    if count > caps.max_entries {
        return Err(format!("the archive has {count} entries; the limit is {}", caps.max_entries));
    }

    let mut names = Vec::with_capacity(count);
    let mut files = HashMap::with_capacity(count);
    let mut p = cd_offset;
    let mut inflated_total: u64 = 0;
    for _ in 0..count {
        if u32at(bytes, p) != Some(CENTRAL_SIG) {
            return Err(format!("bad central directory entry at {p}"));
        }
        let method = u16at(bytes, p + 10).ok_or("truncated central entry")?;
        let crc = u32at(bytes, p + 16).ok_or("truncated central entry")?;
        let comp = u32at(bytes, p + 20).ok_or("truncated central entry")? as usize;
        let uncomp = u32at(bytes, p + 24).ok_or("truncated central entry")? as usize;
        let name_len = u16at(bytes, p + 28).ok_or("truncated central entry")? as usize;
        let extra_len = u16at(bytes, p + 30).ok_or("truncated central entry")? as usize;
        let comment_len = u16at(bytes, p + 32).ok_or("truncated central entry")? as usize;
        let local = u32at(bytes, p + 42).ok_or("truncated central entry")? as usize;
        let name = String::from_utf8_lossy(
            bytes.get(p + 46..p + 46 + name_len).ok_or("truncated central entry name")?,
        )
        .into_owned();

        if name.contains('\0')
            || name.starts_with('/')
            || name.starts_with('\\')
            || name.split(['/', '\\']).any(|c| c == "..")
        {
            return Err(format!("entry {name:?} names a path outside the package"));
        }
        if !want(&name) {
            p += 46 + name_len + extra_len + comment_len;
            continue;
        }
        inflated_total += uncomp as u64;
        if inflated_total > caps.max_inflated {
            return Err(format!(
                "the archive declares more than {} MB of content; the limit is {} MB",
                inflated_total / (1024 * 1024),
                caps.max_inflated / (1024 * 1024)
            ));
        }
        if uncomp > 0 && (comp == 0 || uncomp as u64 / comp.max(1) as u64 > caps.max_ratio) {
            return Err(format!(
                "entry {name:?} declares {uncomp} bytes from {comp} compressed, over the {}:1 limit",
                caps.max_ratio
            ));
        }
        // Follow the offset into the local header to find the data.
        if u32at(bytes, local) != Some(LOCAL_SIG) {
            return Err(format!("entry {name:?} does not point at a local header"));
        }
        let ln = u16at(bytes, local + 26).ok_or("truncated local header")? as usize;
        let le = u16at(bytes, local + 28).ok_or("truncated local header")? as usize;
        let data_at = local + 30 + ln + le;
        let raw = bytes.get(data_at..data_at + comp).ok_or_else(|| format!("entry {name:?} runs off the end"))?;
        let data = match method {
            METHOD_STORE => raw.to_vec(),
            // Bounded by what the directory declared: the size check below
            // then catches a directory that lied in either direction.
            METHOD_DEFLATE => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, uncomp)
                .map_err(|e| format!("entry {name:?} did not inflate: {e:?}"))?,
            other => return Err(format!("entry {name:?} uses compression method {other}, which we do not read")),
        };
        if data.len() != uncomp {
            return Err(format!("entry {name:?} inflated to {} bytes, the directory says {uncomp}", data.len()));
        }
        if crc32(&data) != crc {
            return Err(format!("entry {name:?} fails its CRC"));
        }
        names.push(name.clone());
        files.insert(name, data);
        p += 46 + name_len + extra_len + comment_len;
    }
    Ok(Archive { names, files })
}

// ---------------------------------------------------------------------------

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u16at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// CRC-32/ISO-HDLC, the one zip uses: reflected, polynomial 0xEDB88320.
pub fn crc32(data: &[u8]) -> u32 {
    let table = table();
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn table() -> &'static [u32; 256] {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_published_check_value() {
        // The CRC-32 catalogue's check value: "123456789" -> 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn a_written_archive_reads_back() {
        let mut w = ZipWriter::new();
        w.add("a.txt", b"hello");
        w.add("dir/b.bin", &vec![7u8; 10_000]);
        let bytes = w.finish();
        assert_eq!(&bytes[..4], &0x0403_4b50u32.to_le_bytes(), "starts with a local header");
        let a = read(&bytes).expect("reads back");
        assert_eq!(a.names, vec!["a.txt", "dir/b.bin"]);
        assert_eq!(a.get("a.txt").unwrap(), b"hello");
        assert_eq!(a.get("dir/b.bin").unwrap(), vec![7u8; 10_000].as_slice());
    }

    #[test]
    fn a_compressible_entry_is_deflated_and_a_tiny_one_is_stored() {
        let mut w = ZipWriter::new();
        w.add("big", &vec![0u8; 50_000]);
        w.add("small", b"x");
        let bytes = w.finish();
        assert!(bytes.len() < 5_000, "50 kB of zeros should deflate away: {} bytes", bytes.len());
        assert!(read(&bytes).is_ok());
    }

    #[test]
    fn the_same_content_gives_the_same_bytes() {
        let build = || {
            let mut w = ZipWriter::new();
            w.add("x", b"same");
            w.finish()
        };
        assert_eq!(build(), build(), "timestamps are pinned, so exports are comparable");
    }

    #[test]
    fn a_corrupted_entry_is_refused() {
        let mut w = ZipWriter::new();
        w.add("a", b"hello there");
        let mut bytes = w.finish();
        // Flip a byte inside the stored payload.
        let i = 30 + 1 + 3;
        bytes[i] ^= 0xff;
        let e = read(&bytes).expect_err("must not accept a bad CRC");
        assert!(e.contains("CRC") || e.contains("inflate"), "{e}");
    }
}
