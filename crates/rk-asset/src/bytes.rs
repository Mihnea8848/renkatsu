// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Little-endian numbers, strings and arrays in a chunk: `Out` writes
//! them, `In` reads them back in the same order. Floats keep their exact
//! bits.

use anyhow::{bail, Result};

/// Bytes being written.
#[derive(Debug, Default)]
pub struct Out {
    buf: Vec<u8>,
}

impl Out {
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// A count or an index, as a u32.
    pub fn len_of(&mut self, n: usize) {
        self.u32(u32::try_from(n).expect("fewer than 2^32 items"));
    }

    /// An index that may be missing: 0 for none, else one more than it.
    pub fn index(&mut self, i: Option<usize>) {
        self.len_of(i.map_or(0, |i| i + 1));
    }

    pub fn floats(&mut self, v: &[f32]) {
        for f in v {
            self.f32(*f);
        }
    }

    pub fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }

    /// Bytes with their length before them.
    pub fn bytes(&mut self, b: &[u8]) {
        self.len_of(b.len());
        self.buf.extend_from_slice(b);
    }

    /// A list of float arrays with its length before it.
    pub fn arrays<const N: usize>(&mut self, v: &[[f32; N]]) {
        self.len_of(v.len());
        self.buf.reserve(v.len() * N * 4);
        for a in v {
            self.floats(a);
        }
    }

    pub fn u32s(&mut self, v: &[u32]) {
        self.len_of(v.len());
        self.buf.reserve(v.len() * 4);
        for n in v {
            self.u32(*n);
        }
    }
}

/// Bytes being read.
#[derive(Debug)]
pub struct In<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> In<'a> {
    pub fn new(data: &'a [u8]) -> In<'a> {
        In { data, at: 0 }
    }

    /// Whether everything has been read.
    pub fn done(&self) -> bool {
        self.at == self.data.len()
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        match self.at.checked_add(n).filter(|end| *end <= self.data.len()) {
            Some(end) => {
                let out = &self.data[self.at..end];
                self.at = end;
                Ok(out)
            }
            None => bail!("the chunk ends too soon"),
        }
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("N bytes"))
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => bail!("{other} is not a yes or no"),
        }
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    pub fn len_of(&mut self) -> Result<usize> {
        Ok(self.u32()? as usize)
    }

    pub fn index(&mut self) -> Result<Option<usize>> {
        Ok(self.len_of()?.checked_sub(1))
    }

    pub fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for f in &mut out {
            *f = self.f32()?;
        }
        Ok(out)
    }

    pub fn str(&mut self) -> Result<&'a str> {
        Ok(std::str::from_utf8(self.bytes()?)?)
    }

    pub fn bytes(&mut self) -> Result<&'a [u8]> {
        let n = self.len_of()?;
        self.take(n)
    }

    pub fn arrays<const N: usize>(&mut self) -> Result<Vec<[f32; N]>> {
        let n = self.len_of()?;
        let raw = self.take(n.saturating_mul(N * 4))?;
        Ok(raw
            .chunks_exact(N * 4)
            .map(|c| std::array::from_fn(|i| f32::from_le_bytes(c[i * 4..i * 4 + 4].try_into().expect("four bytes"))))
            .collect())
    }

    pub fn u32s(&mut self) -> Result<Vec<u32>> {
        let n = self.len_of()?;
        let raw = self.take(n.saturating_mul(4))?;
        Ok(raw
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().expect("four bytes")))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_reads_back_bit_for_bit() {
        let mut o = Out::default();
        o.u8(7);
        o.bool(true);
        o.u32(0xDEAD_BEEF);
        o.i32(-3);
        o.u64(1 << 40);
        o.f32(-0.0);
        o.f32(f32::from_bits(0x7FC0_1234));
        o.index(None);
        o.index(Some(4));
        o.str("屋上");
        o.arrays(&[[1.0f32, 2.0, 3.0], [4.0, 5.0, f32::MIN_POSITIVE]]);
        o.u32s(&[1, 2, 3]);
        let bytes = o.into_vec();
        let mut i = In::new(&bytes);
        assert_eq!(i.u8().unwrap(), 7);
        assert!(i.bool().unwrap());
        assert_eq!(i.u32().unwrap(), 0xDEAD_BEEF);
        assert_eq!(i.i32().unwrap(), -3);
        assert_eq!(i.u64().unwrap(), 1 << 40);
        assert_eq!(i.f32().unwrap().to_bits(), (-0.0f32).to_bits());
        assert_eq!(i.f32().unwrap().to_bits(), 0x7FC0_1234);
        assert_eq!(i.index().unwrap(), None);
        assert_eq!(i.index().unwrap(), Some(4));
        assert_eq!(i.str().unwrap(), "屋上");
        assert_eq!(
            i.arrays::<3>().unwrap(),
            vec![[1.0, 2.0, 3.0], [4.0, 5.0, f32::MIN_POSITIVE]]
        );
        assert_eq!(i.u32s().unwrap(), vec![1, 2, 3]);
        assert!(i.done());
        assert!(i.u8().is_err());
    }

    #[test]
    fn a_length_past_the_end_fails() {
        let mut o = Out::default();
        o.u32(1_000_000);
        let bytes = o.into_vec();
        assert!(In::new(&bytes).arrays::<3>().is_err());
        assert!(In::new(&bytes).str().is_err());
    }
}
