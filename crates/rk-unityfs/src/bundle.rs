// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! UnityFS asset bundle container: header, block table, directory, and
//! on-demand block decompression.

use crate::io::Cursor;
use crate::{Error, Result};
use std::path::Path;
use std::sync::Arc;

enum Source {
    Map(memmap2::Mmap),
    Mem(Vec<u8>),
}

impl Source {
    fn bytes(&self) -> &[u8] {
        match self {
            Source::Map(m) => m,
            Source::Mem(v) => v,
        }
    }
}

#[derive(Debug, Clone)]
struct Block {
    comp_off: u64,
    comp_size: u32,
    uncomp_off: u64,
    uncomp_size: u32,
    compression: u8,
}

/// One file stored inside the bundle (a serialized file or a `.resS` blob).
#[derive(Debug, Clone)]
pub struct Node {
    pub path: String,
    pub offset: u64,
    pub size: u64,
    pub flags: u32,
}

impl Node {
    /// Flag bit 2 marks serialized (object) files; resource blobs lack it.
    pub fn is_serialized(&self) -> bool {
        self.flags & 4 != 0
    }
}

pub struct Bundle {
    src: Source,
    blocks: Vec<Block>,
    pub nodes: Vec<Node>,
    pub format: u32,
    pub unity_version: String,
}

fn decompress(kind: u8, input: &[u8], out_len: usize) -> Result<Vec<u8>> {
    match kind {
        0 => Ok(input.to_vec()),
        1 => {
            let mut out = Vec::with_capacity(out_len);
            let opts = lzma_rs::decompress::Options {
                unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(Some(out_len as u64)),
                memlimit: None,
                allow_incomplete: true,
            };
            lzma_rs::lzma_decompress_with_options(&mut std::io::Cursor::new(input), &mut out, &opts)
                .map_err(|e| Error::Compression(format!("lzma: {e:?}")))?;
            Ok(out)
        }
        2 | 3 => lz4_flex::block::decompress(input, out_len).map_err(|e| Error::Compression(format!("lz4: {e}"))),
        k => Err(Error::Compression(format!("unknown compression type {k}"))),
    }
}

impl Bundle {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        // Safety: the map is read-only; game files are not expected to change
        // underneath a running tool.
        let map = unsafe { memmap2::Mmap::map(&file)? };
        Self::new(Source::Map(map))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::new(Source::Mem(bytes))
    }

    /// True when `bytes` starts with a UnityFS signature.
    pub fn is_bundle(bytes: &[u8]) -> bool {
        bytes.starts_with(b"UnityFS\0")
    }

    fn new(src: Source) -> Result<Self> {
        let data = src.bytes();
        let mut c = Cursor::new(data, true);
        let sig = c.cstr()?;
        if sig != "UnityFS" {
            return Err(Error::Format(format!("unsupported bundle signature {sig:?}")));
        }
        let format = c.u32()?;
        let _player = c.cstr()?;
        let unity_version = c.cstr()?;
        let _size = c.u64()?;
        let comp_info = c.u32()? as usize;
        let uncomp_info = c.u32()? as usize;
        let flags = c.u32()?;
        if format >= 7 {
            c.align(16);
        }
        let info_at_end = flags & 0x80 != 0;
        let info_pos = if info_at_end {
            data.len().checked_sub(comp_info).ok_or(Error::Eof)?
        } else {
            c.pos()
        };
        let info_raw = data.get(info_pos..info_pos + comp_info).ok_or(Error::Eof)?;
        let info = decompress((flags & 0x3f) as u8, info_raw, uncomp_info)?;
        let mut data_pos = if info_at_end { c.pos() } else { c.pos() + comp_info } as u64;
        if flags & 0x200 != 0 {
            data_pos = (data_pos + 15) & !15;
        }

        let mut i = Cursor::new(&info, true);
        i.skip(16)?;
        let block_count = i.u32()? as usize;
        let mut blocks = Vec::with_capacity(block_count);
        let (mut comp_off, mut uncomp_off) = (data_pos, 0u64);
        for _ in 0..block_count {
            let uncomp_size = i.u32()?;
            let comp_size = i.u32()?;
            let bflags = i.u16()?;
            blocks.push(Block {
                comp_off,
                comp_size,
                uncomp_off,
                uncomp_size,
                compression: (bflags & 0x3f) as u8,
            });
            comp_off += comp_size as u64;
            uncomp_off += uncomp_size as u64;
        }
        let node_count = i.u32()? as usize;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let offset = i.u64()?;
            let size = i.u64()?;
            let flags = i.u32()?;
            let path = i.cstr()?;
            nodes.push(Node {
                path,
                offset,
                size,
                flags,
            });
        }
        Ok(Self {
            src,
            blocks,
            nodes,
            format,
            unity_version,
        })
    }

    /// Reads `len` bytes at `offset` of the uncompressed bundle data,
    /// decompressing only the blocks that cover the range.
    pub fn read_range(&self, offset: u64, len: u64) -> Result<Vec<u8>> {
        let data = self.src.bytes();
        let end = offset + len;
        let mut out = Vec::with_capacity(len as usize);
        let first = self
            .blocks
            .partition_point(|b| b.uncomp_off + b.uncomp_size as u64 <= offset);
        for b in &self.blocks[first..] {
            if b.uncomp_off >= end {
                break;
            }
            let raw = data
                .get(b.comp_off as usize..(b.comp_off + b.comp_size as u64) as usize)
                .ok_or(Error::Eof)?;
            let lo = offset.saturating_sub(b.uncomp_off) as usize;
            let hi = (end.min(b.uncomp_off + b.uncomp_size as u64) - b.uncomp_off) as usize;
            if b.compression == 0 {
                out.extend_from_slice(raw.get(lo..hi).ok_or(Error::Eof)?);
            } else {
                let plain = decompress(b.compression, raw, b.uncomp_size as usize)?;
                out.extend_from_slice(plain.get(lo..hi).ok_or(Error::Eof)?);
            }
        }
        if out.len() as u64 != len {
            return Err(Error::Eof);
        }
        Ok(out)
    }

    pub fn read_node(&self, node: &Node) -> Result<Arc<[u8]>> {
        Ok(self.read_range(node.offset, node.size)?.into())
    }

    pub fn node(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.path.eq_ignore_ascii_case(name))
    }
}
