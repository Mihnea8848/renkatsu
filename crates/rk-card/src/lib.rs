// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Koikatsu character card and coordinate file parser.
//!
//! A card is a PNG image followed by a binary trailer:
//!
//! ```text
//! i32      product number (100)
//! string   marker, e.g. 【KoiKatuChara】        (.NET 7-bit length prefix)
//! string   file version
//! i32+data face thumbnail PNG
//! i32+data block header (MessagePack: lstInfo[{name, version, pos, size}])
//! i64      total size of the block data
//! data     blocks: Custom, Coordinate, Parameter, Status, KKEx (plugins)
//! ```

mod model;
mod reader;

pub use model::*;
pub use rmpv::Value;

use reader::Reader;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum CardError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a PNG file")]
    NotPng,
    #[error("PNG has no card data after it")]
    NoTrailer,
    #[error("unexpected end of data while reading {0}")]
    Eof(&'static str),
    #[error("unsupported product number {0}")]
    Product(i32),
    #[error("unknown card marker {0:?}")]
    Marker(String),
    #[error("block {0} is missing")]
    MissingBlock(&'static str),
    #[error("block {block}: {source}")]
    Decode {
        block: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

pub type Result<T> = std::result::Result<T, CardError>;

fn dec<E: std::error::Error + Send + Sync + 'static>(block: &'static str) -> impl FnOnce(E) -> CardError {
    move |e| CardError::Decode {
        block,
        source: Box::new(e),
    }
}

/// Which game wrote the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Koikatsu,
    KoikatsuParty,
    KoikatsuPartySpecial,
    EmotionCreators,
    KoikatsuSunshine,
}

impl CardKind {
    fn from_marker(m: &str) -> Option<Self> {
        Some(match m {
            "【KoiKatuChara】" => Self::Koikatsu,
            "【KoiKatuCharaS】" => Self::KoikatsuParty,
            "【KoiKatuCharaSP】" => Self::KoikatsuPartySpecial,
            "【EroMakeChara】" => Self::EmotionCreators,
            "【KoiKatuCharaSun】" => Self::KoikatsuSunshine,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub name: String,
    pub version: String,
    pub pos: u64,
    pub size: u64,
}

/// Data a plugin stored in the card through ExtensibleSaveFormat.
#[derive(Debug, Clone, Default)]
pub struct PluginData {
    pub version: i64,
    pub data: BTreeMap<String, Value>,
}

impl PluginData {
    /// Many plugins store a nested MessagePack document as a byte blob.
    pub fn nested(&self, key: &str) -> Option<Value> {
        match self.data.get(key)? {
            Value::Binary(b) => rmpv::decode::read_value(&mut &b[..]).ok(),
            _ => None,
        }
    }
    pub fn bytes(&self, key: &str) -> Option<&[u8]> {
        match self.data.get(key)? {
            Value::Binary(b) => Some(b),
            _ => None,
        }
    }
}

pub type ExtData = BTreeMap<String, PluginData>;

#[derive(Debug, Clone)]
pub struct Card {
    pub kind: CardKind,
    pub product_no: i32,
    pub version: String,
    /// Length of the leading card image, so callers can slice it out.
    pub image_len: usize,
    pub face_png: Vec<u8>,
    pub blocks: Vec<BlockInfo>,
    pub face: Face,
    pub body: Body,
    pub hair: Hair,
    pub coordinates: Vec<Coordinate>,
    pub parameter: Value,
    pub status: Value,
    pub ext: ExtData,
}

impl Card {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::parse(&std::fs::read(path)?)
    }

    pub fn parse(file: &[u8]) -> Result<Self> {
        let image_len = png_len(file)?;
        if image_len >= file.len() {
            return Err(CardError::NoTrailer);
        }
        let mut r = Reader::new(&file[image_len..]);
        let product_no = r.i32("product number")?;
        if product_no > 100 {
            return Err(CardError::Product(product_no));
        }
        let marker = r.string("marker")?;
        let kind = CardKind::from_marker(&marker).ok_or(CardError::Marker(marker))?;
        let version = r.string("version")?;
        let face_png = r.len_bytes("face image")?.to_vec();
        let header_bytes = r.len_bytes("block header")?;
        let data_len = r.i64("data size")? as usize;
        let data = r.bytes(data_len.min(r.remaining()), "block data")?;

        let header: BlockHeader = rmp_serde::from_slice(header_bytes).map_err(dec("header"))?;
        let blocks: Vec<BlockInfo> = header
            .lst_info
            .into_iter()
            .map(|i| BlockInfo {
                name: i.name,
                version: i.version,
                pos: i.pos as u64,
                size: i.size as u64,
            })
            .collect();
        let block = |name: &'static str| -> Result<&[u8]> {
            let b = blocks
                .iter()
                .find(|b| b.name == name)
                .ok_or(CardError::MissingBlock(name))?;
            data.get(b.pos as usize..(b.pos + b.size) as usize)
                .ok_or(CardError::Eof(name))
        };

        let mut c = Reader::new(block("Custom")?);
        let face: Face = rmp_serde::from_slice(c.len_bytes("face")?).map_err(dec("Custom/face"))?;
        let body: Body = rmp_serde::from_slice(c.len_bytes("body")?).map_err(dec("Custom/body"))?;
        let hair: Hair = rmp_serde::from_slice(c.len_bytes("hair")?).map_err(dec("Custom/hair"))?;

        let coord_list: Vec<serde_bytes_buf::ByteBuf> =
            rmp_serde::from_slice(block("Coordinate")?).map_err(dec("Coordinate"))?;
        let coordinates = coord_list
            .iter()
            .map(|b| Coordinate::parse_bytes(&b.0))
            .collect::<Result<Vec<_>>>()?;

        let parameter = read_value(block("Parameter")?, "Parameter")?;
        // Optional blocks may be absent from the header, but one that is
        // listed and cut short is a damaged file.
        let status = match block("Status") {
            Ok(b) => read_value(b, "Status")?,
            Err(CardError::MissingBlock(_)) => Value::Nil,
            Err(e) => return Err(e),
        };
        let ext = match block("KKEx") {
            Ok(b) => parse_ext(b)?,
            Err(CardError::MissingBlock(_)) => ExtData::new(),
            Err(e) => return Err(e),
        };

        Ok(Card {
            kind,
            product_no,
            version,
            image_len,
            face_png,
            blocks,
            face,
            body,
            hair,
            coordinates,
            parameter,
            status,
            ext,
        })
    }

    pub fn param_str(&self, key: &str) -> Option<&str> {
        map_get(&self.parameter, key)?.as_str()
    }
    pub fn param_int(&self, key: &str) -> Option<i64> {
        map_get(&self.parameter, key)?.as_i64()
    }
    /// 0 = male, 1 = female.
    pub fn sex(&self) -> i64 {
        self.param_int("sex").unwrap_or(1)
    }
    pub fn full_name(&self) -> String {
        let l = self.param_str("lastname").unwrap_or("");
        let f = self.param_str("firstname").unwrap_or("");
        format!("{l} {f}").trim().to_string()
    }
}

/// A standalone outfit file (`UserData/coordinate/*.png`).
#[derive(Debug, Clone)]
pub struct CoordinateFile {
    pub image_len: usize,
    pub version: String,
    pub name: String,
    pub coordinate: Coordinate,
    pub ext: ExtData,
}

impl CoordinateFile {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::parse(&std::fs::read(path)?)
    }

    pub fn parse(file: &[u8]) -> Result<Self> {
        let image_len = png_len(file)?;
        if image_len >= file.len() {
            return Err(CardError::NoTrailer);
        }
        let mut r = Reader::new(&file[image_len..]);
        let product_no = r.i32("product number")?;
        if product_no > 100 {
            return Err(CardError::Product(product_no));
        }
        let marker = r.string("marker")?;
        if marker != "【KoiKatuClothes】" {
            return Err(CardError::Marker(marker));
        }
        let version = r.string("version")?;
        let name = r.string("name")?;
        let coordinate = Coordinate::parse_bytes(r.len_bytes("coordinate")?)?;
        // ExtensibleSaveFormat appends: "KKEx", i32 version, i32 length, data.
        let mut ext = ExtData::new();
        if r.remaining() > 0 {
            if let Ok(tag) = r.string("ext marker") {
                if tag == "KKEx" {
                    let _ver = r.i32("ext version")?;
                    ext = parse_ext(r.len_bytes("ext data")?)?;
                }
            }
        }
        Ok(Self {
            image_len,
            version,
            name,
            coordinate,
            ext,
        })
    }
}

impl Coordinate {
    pub(crate) fn parse_bytes(b: &[u8]) -> Result<Self> {
        let mut r = Reader::new(b);
        let clothes: Clothes = rmp_serde::from_slice(r.len_bytes("clothes")?).map_err(dec("Coordinate/clothes"))?;
        let accessory: Accessories =
            rmp_serde::from_slice(r.len_bytes("accessory")?).map_err(dec("Coordinate/accessory"))?;
        let enable_makeup = r.u8("enableMakeup")? != 0;
        let makeup: Makeup = rmp_serde::from_slice(r.len_bytes("makeup")?).map_err(dec("Coordinate/makeup"))?;
        Ok(Coordinate {
            clothes,
            accessory,
            enable_makeup,
            makeup,
        })
    }
}

fn read_value(mut b: &[u8], block: &'static str) -> Result<Value> {
    rmpv::decode::read_value(&mut b).map_err(dec(block))
}

fn parse_ext(b: &[u8]) -> Result<ExtData> {
    let v = read_value(b, "KKEx")?;
    let mut out = ExtData::new();
    let Value::Map(entries) = v else { return Ok(out) };
    for (k, v) in entries {
        let Some(key) = k.as_str() else { continue };
        // PluginData is [version, data]; data may be nil.
        let mut pd = PluginData::default();
        if let Value::Array(a) = v {
            pd.version = a.first().and_then(Value::as_i64).unwrap_or(0);
            if let Some(Value::Map(m)) = a.into_iter().nth(1) {
                for (dk, dv) in m {
                    if let Some(dk) = dk.as_str() {
                        pd.data.insert(dk.to_string(), dv);
                    }
                }
            }
        }
        out.insert(key.to_string(), pd);
    }
    Ok(out)
}

/// Decodes a blob written by MessagePack-CSharp's LZ4 serializer, which
/// many plugins use: either plain MessagePack, or extension type 99 holding
/// the uncompressed size followed by an LZ4 block.
pub fn lz4_msgpack(bytes: &[u8]) -> Option<Value> {
    let v = rmpv::decode::read_value(&mut &bytes[..]).ok()?;
    let Value::Ext(99, payload) = v else { return Some(v) };
    let mut rest = &payload[..];
    let size = rmpv::decode::read_value(&mut rest).ok()?.as_u64()? as usize;
    let plain = lz4_flex::block::decompress(rest, size).ok()?;
    rmpv::decode::read_value(&mut &plain[..]).ok()
}

impl PluginData {
    /// A value stored through the LZ4 MessagePack serializer.
    pub fn lz4(&self, key: &str) -> Option<Value> {
        lz4_msgpack(self.bytes(key)?)
    }
}

/// Looks up a string key in a MessagePack map value.
pub fn map_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

/// Length of the PNG at the start of `file`, found by walking its chunks.
/// Zero when the file does not start with a PNG.
pub fn png_len(file: &[u8]) -> Result<usize> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !file.starts_with(SIG) {
        // Exported cards have no image; the game accepts those too.
        return Ok(0);
    }
    let mut pos = SIG.len();
    loop {
        let head = file.get(pos..pos + 8).ok_or(CardError::NotPng)?;
        let len = u32::from_be_bytes(head[..4].try_into().unwrap()) as usize;
        let end = pos + 12 + len;
        if end > file.len() {
            return Err(CardError::NotPng);
        }
        if &head[4..8] == b"IEND" {
            return Ok(end);
        }
        pos = end;
    }
}

mod serde_bytes_buf {
    use serde::de::{Deserialize, Deserializer, SeqAccess, Visitor};
    use std::fmt;

    /// Byte buffer that deserializes from MessagePack `bin`.
    pub struct ByteBuf(pub Vec<u8>);

    impl<'de> Deserialize<'de> for ByteBuf {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = ByteBuf;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("bytes")
                }
                fn visit_bytes<E>(self, v: &[u8]) -> Result<ByteBuf, E> {
                    Ok(ByteBuf(v.to_vec()))
                }
                fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<ByteBuf, E> {
                    Ok(ByteBuf(v))
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ByteBuf, A::Error> {
                    let mut out = Vec::new();
                    while let Some(b) = seq.next_element::<u8>()? {
                        out.push(b);
                    }
                    Ok(ByteBuf(out))
                }
            }
            d.deserialize_byte_buf(V)
        }
    }
}
