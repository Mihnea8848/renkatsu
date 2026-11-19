// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Renkatsu's asset file: one file holding named chunks, with a table of
//! them at the end so that any chunk can be read alone, without the
//! others. A chunk is stored as it is or packed with zstd, whichever is
//! smaller by enough to be worth unpacking.
//!
//! ```text
//! header   64 bytes: magic, container version, kind, kind's version,
//!          chunk count, where the table starts and how long it is
//! chunks   each starting on a 16 byte boundary
//! table    per chunk: name, offset, stored length, unpacked length,
//!          packing
//! ```
//!
//! Numbers are little-endian. The kind says what the file holds (a map,
//! later a baked character, an item or an animation) and has a version of
//! its own, for the layout of its chunks; the container's version only
//! changes with the header and table.

pub mod bytes;

use anyhow::{bail, ensure, Context, Result};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The first bytes of every asset file.
pub const MAGIC: [u8; 8] = *b"RENKATSU";
/// The version of the header and chunk table this build writes and reads.
pub const CONTAINER: u32 = 1;
const HEADER: usize = 64;
const ALIGN: u64 = 16;
/// The zstd level chunks are packed at: near the smallest zstd makes, at a
/// speed that keeps an import short. Unpacking is as fast at any level.
pub const LEVEL: i32 = 9;

/// How a chunk is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    Raw,
    Zstd,
}

impl Packing {
    fn code(self) -> u8 {
        match self {
            Packing::Raw => 0,
            Packing::Zstd => 1,
        }
    }

    fn of(code: u8) -> Result<Packing> {
        Ok(match code {
            0 => Packing::Raw,
            1 => Packing::Zstd,
            other => bail!("unknown chunk packing {other}"),
        })
    }
}

/// How to store a chunk being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pack {
    /// As it is: data that does not pack, such as block-compressed
    /// textures, or that is read in place.
    Raw,
    /// Packed with zstd whatever it saves.
    Zstd,
    /// Packed when that saves at least a sixteenth, else as it is.
    Auto,
}

/// A chunk made ready to write: packing can run on many threads at once,
/// writing then only copies.
#[derive(Debug)]
pub struct Packed {
    name: String,
    data: Vec<u8>,
    size: u64,
    packing: Packing,
}

/// Packs `data` for the chunk `name`.
pub fn pack(name: &str, data: &[u8], how: Pack) -> Result<Packed> {
    let raw = || Packed {
        name: name.to_owned(),
        data: data.to_vec(),
        size: data.len() as u64,
        packing: Packing::Raw,
    };
    if how == Pack::Raw || data.is_empty() {
        return Ok(raw());
    }
    let mut encoder = zstd::stream::Encoder::new(Vec::with_capacity(data.len() / 2), LEVEL)?;
    encoder.include_checksum(true)?;
    encoder.set_pledged_src_size(Some(data.len() as u64))?;
    encoder.write_all(data)?;
    let packed = encoder.finish()?;
    if how == Pack::Auto && packed.len() as u64 > data.len() as u64 - data.len() as u64 / 16 {
        return Ok(raw());
    }
    Ok(Packed {
        name: name.to_owned(),
        data: packed,
        size: data.len() as u64,
        packing: Packing::Zstd,
    })
}

/// A chunk as the table lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub offset: u64,
    /// Its length in the file.
    pub stored: u64,
    /// Its length unpacked.
    pub size: u64,
    pub packing: Packing,
}

/// What the header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub container: u32,
    /// What the file holds, such as `*b"MAP\0"`.
    pub kind: [u8; 4],
    /// The version of the kind's layout.
    pub version: u32,
    count: u32,
    table: u64,
    table_len: u64,
}

impl Header {
    fn bytes(&self) -> [u8; HEADER] {
        let mut h = [0u8; HEADER];
        h[0..8].copy_from_slice(&MAGIC);
        h[8..12].copy_from_slice(&self.container.to_le_bytes());
        h[12..16].copy_from_slice(&self.kind);
        h[16..20].copy_from_slice(&self.version.to_le_bytes());
        h[20..24].copy_from_slice(&self.count.to_le_bytes());
        h[24..32].copy_from_slice(&self.table.to_le_bytes());
        h[32..40].copy_from_slice(&self.table_len.to_le_bytes());
        h
    }

    /// Reads a header, or `None` when the bytes are not an asset file's.
    fn parse(h: &[u8]) -> Option<Header> {
        if h.len() < HEADER || h[0..8] != MAGIC {
            return None;
        }
        let u32_at = |i: usize| u32::from_le_bytes(h[i..i + 4].try_into().expect("four bytes"));
        let u64_at = |i: usize| u64::from_le_bytes(h[i..i + 8].try_into().expect("eight bytes"));
        Some(Header {
            container: u32_at(8),
            kind: h[12..16].try_into().expect("four bytes"),
            version: u32_at(16),
            count: u32_at(20),
            table: u64_at(24),
            table_len: u64_at(32),
        })
    }
}

/// The header of the file at `path`, or `None` when it is not an asset
/// file (an older text sidecar, say).
pub fn peek(path: &Path) -> Result<Option<Header>> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut h = [0u8; HEADER];
    let mut got = 0;
    while got < HEADER {
        let n = file.read(&mut h[got..])?;
        if n == 0 {
            return Ok(None);
        }
        got += n;
    }
    Ok(Header::parse(&h))
}

/// Writes an asset file. It is written beside its place under another
/// name and moved there by `finish`, so that a file cut short by an error
/// never takes the place of a whole one.
pub struct Writer {
    /// Taken by `finish`.
    out: Option<BufWriter<File>>,
    at: u64,
    kind: [u8; 4],
    version: u32,
    table: Vec<Entry>,
    temp: PathBuf,
    path: PathBuf,
}

impl Writer {
    pub fn create(path: &Path, kind: [u8; 4], version: u32) -> Result<Writer> {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".part");
        let temp = path.with_file_name(name);
        let file = File::create(&temp).with_context(|| format!("writing {}", temp.display()))?;
        let mut out = BufWriter::with_capacity(1 << 20, file);
        // The header is written again by `finish`, once the table is known.
        out.write_all(&[0u8; HEADER])?;
        Ok(Writer {
            out: Some(out),
            at: HEADER as u64,
            kind,
            version,
            table: Vec::new(),
            temp,
            path: path.to_owned(),
        })
    }

    /// Adds a chunk. Names are unique within a file.
    pub fn add(&mut self, name: &str, data: &[u8], how: Pack) -> Result<()> {
        self.add_packed(pack(name, data, how)?)
    }

    /// Adds a chunk packed beforehand.
    pub fn add_packed(&mut self, chunk: Packed) -> Result<()> {
        ensure!(chunk.name.len() <= u16::MAX as usize, "chunk name too long");
        ensure!(
            !self.table.iter().any(|e| e.name == chunk.name),
            "two chunks named {}",
            chunk.name
        );
        let pad = (ALIGN - self.at % ALIGN) % ALIGN;
        let out = self.out.as_mut().context("the file is finished")?;
        out.write_all(&[0u8; ALIGN as usize][..pad as usize])?;
        self.at += pad;
        out.write_all(&chunk.data)?;
        self.table.push(Entry {
            name: chunk.name,
            offset: self.at,
            stored: chunk.data.len() as u64,
            size: chunk.size,
            packing: chunk.packing,
        });
        self.at += chunk.data.len() as u64;
        Ok(())
    }

    /// Writes the table and the header, and puts the file in its place.
    /// Returns its length.
    pub fn finish(mut self) -> Result<u64> {
        let mut t = bytes::Out::default();
        for e in &self.table {
            t.str(&e.name);
            t.u64(e.offset);
            t.u64(e.stored);
            t.u64(e.size);
            t.u8(e.packing.code());
        }
        let table = t.into_vec();
        let mut out = self.out.take().context("the file is finished")?;
        out.write_all(&table)?;
        let header = Header {
            container: CONTAINER,
            kind: self.kind,
            version: self.version,
            count: self.table.len() as u32,
            table: self.at,
            table_len: table.len() as u64,
        };
        let length = self.at + table.len() as u64;
        let mut file = out.into_inner().map_err(|e| e.into_error())?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header.bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&self.temp, &self.path).with_context(|| format!("writing {}", self.path.display()))?;
        self.temp.clear();
        Ok(length)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Left unfinished: the part written goes.
        if !self.temp.as_os_str().is_empty() {
            self.out = None;
            let _ = std::fs::remove_file(&self.temp);
        }
    }
}

/// An asset file open for reading. Chunks are read on demand, from any
/// number of threads at once.
#[derive(Debug)]
pub struct Reader {
    file: File,
    header: Header,
    table: Vec<Entry>,
    length: u64,
}

impl Reader {
    /// Opens an asset file and reads its table. Fails for a file that is
    /// not one, or that a newer Renkatsu wrote with a container this build
    /// does not know.
    pub fn open(path: &Path) -> Result<Reader> {
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let length = file.metadata()?.len();
        let mut h = [0u8; HEADER];
        file.read_exact(&mut h)
            .with_context(|| format!("{} is not a Renkatsu asset file", path.display()))?;
        let header = Header::parse(&h).with_context(|| format!("{} is not a Renkatsu asset file", path.display()))?;
        ensure!(
            header.container <= CONTAINER,
            "{} was written by a newer Renkatsu (container {})",
            path.display(),
            header.container
        );
        ensure!(
            header.table >= HEADER as u64 && header.table.checked_add(header.table_len) == Some(length),
            "{} is cut short or damaged",
            path.display()
        );
        let mut raw = vec![0u8; header.table_len as usize];
        read_at(&file, header.table, &mut raw)?;
        let mut t = bytes::In::new(&raw);
        let mut table = Vec::with_capacity(header.count as usize);
        for _ in 0..header.count {
            let e = Entry {
                name: t.str()?.to_owned(),
                offset: t.u64()?,
                stored: t.u64()?,
                size: t.u64()?,
                packing: Packing::of(t.u8()?)?,
            };
            ensure!(
                e.offset.checked_add(e.stored).is_some_and(|end| end <= header.table),
                "chunk {} of {} lies outside it",
                e.name,
                path.display()
            );
            table.push(e);
        }
        Ok(Reader {
            file,
            header,
            table,
            length,
        })
    }

    pub fn header(&self) -> Header {
        self.header
    }

    /// The file's length in bytes.
    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn entries(&self) -> &[Entry] {
        &self.table
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.table.iter().find(|e| e.name == name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.entry(name).is_some()
    }

    /// A chunk, unpacked.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let e = self.entry(name).with_context(|| format!("no chunk {name}"))?;
        let mut stored = vec![0u8; e.stored as usize];
        read_at(&self.file, e.offset, &mut stored)?;
        match e.packing {
            Packing::Raw => Ok(stored),
            Packing::Zstd => {
                let mut out = Vec::with_capacity(e.size as usize);
                zstd::stream::Decoder::new(stored.as_slice())?
                    .read_to_end(&mut out)
                    .with_context(|| format!("unpacking chunk {name}"))?;
                ensure!(out.len() as u64 == e.size, "chunk {name} unpacks to the wrong length");
                Ok(out)
            }
        }
    }
}

#[cfg(unix)]
fn read_at(file: &File, offset: u64, buf: &mut [u8]) -> Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset).context("reading past the end")
}

#[cfg(windows)]
fn read_at(file: &File, mut offset: u64, mut buf: &mut [u8]) -> Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = file.seek_read(buf, offset)?;
        ensure!(n > 0, "reading past the end");
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("renkatsu-asset-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn chunks_read_back_alone_and_in_any_order() {
        let dir = scratch("chunks");
        let path = dir.join("a.rkmap");
        let text = b"name = \"Rooftop\"\n".repeat(50);
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        let noise: Vec<u8> = (0..5000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect();
        let mut w = Writer::create(&path, *b"TEST", 3).unwrap();
        w.add("meta", &text, Pack::Auto).unwrap();
        w.add("noise", &noise, Pack::Auto).unwrap();
        w.add("empty", &[], Pack::Zstd).unwrap();
        w.add("raw", b"abc", Pack::Raw).unwrap();
        let length = w.finish().unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), length);
        assert!(!dir.join("a.rkmap.part").exists());

        let r = Reader::open(&path).unwrap();
        assert_eq!(r.header().kind, *b"TEST");
        assert_eq!(r.header().version, 3);
        assert_eq!(r.entries().len(), 4);
        // Repeated text packs; noise does not, and is kept as it is.
        assert_eq!(r.entry("meta").unwrap().packing, Packing::Zstd);
        assert!(r.entry("meta").unwrap().stored < text.len() as u64 / 4);
        assert_eq!(r.entry("noise").unwrap().packing, Packing::Raw);
        assert_eq!(r.read("raw").unwrap(), b"abc");
        assert_eq!(r.read("noise").unwrap(), noise);
        assert_eq!(r.read("meta").unwrap(), text);
        assert!(r.read("empty").unwrap().is_empty());
        assert!(r.read("missing").is_err());
        for e in r.entries() {
            assert_eq!(e.offset % ALIGN, 0, "{} is aligned", e.name);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_files_are_told_apart() {
        let dir = scratch("other");
        let text = dir.join("old.rkmap");
        std::fs::write(&text, "format = 1\nname = \"Rooftop\"\n".repeat(4)).unwrap();
        assert_eq!(peek(&text).unwrap(), None);
        assert!(Reader::open(&text).is_err());
        let short = dir.join("short.rkmap");
        std::fs::write(&short, b"REN").unwrap();
        assert_eq!(peek(&short).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_file_is_refused() {
        let dir = scratch("damaged");
        let path = dir.join("a.rkmap");
        let mut w = Writer::create(&path, *b"TEST", 1).unwrap();
        w.add("meta", &b"x".repeat(1000), Pack::Zstd).unwrap();
        w.finish().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        // Cut short.
        std::fs::write(&path, &bytes[..bytes.len() - 3]).unwrap();
        assert!(Reader::open(&path).is_err());
        // A flipped byte in a packed chunk fails its checksum.
        bytes[HEADER + 6] ^= 0x55;
        std::fs::write(&path, &bytes).unwrap();
        assert!(Reader::open(&path).unwrap().read("meta").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_container_is_refused() {
        let dir = scratch("newer");
        let path = dir.join("a.rkmap");
        Writer::create(&path, *b"TEST", 1).unwrap().finish().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..12].copy_from_slice(&(CONTAINER + 1).to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let e = Reader::open(&path).unwrap_err();
        assert!(format!("{e:#}").contains("newer"), "{e:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unfinished_file_leaves_nothing() {
        let dir = scratch("unfinished");
        let path = dir.join("a.rkmap");
        {
            let mut w = Writer::create(&path, *b"TEST", 1).unwrap();
            w.add("meta", b"x", Pack::Raw).unwrap();
        }
        assert!(!path.exists());
        assert!(!dir.join("a.rkmap.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
