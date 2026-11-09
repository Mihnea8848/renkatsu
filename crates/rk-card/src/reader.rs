// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{CardError, Result};

/// Little-endian cursor matching .NET `BinaryReader`.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn bytes(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(CardError::Eof(what))?;
        let s = self.buf.get(self.pos..end).ok_or(CardError::Eof(what))?;
        self.pos = end;
        Ok(s)
    }

    pub fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.bytes(1, what)?[0])
    }

    pub fn i32(&mut self, what: &'static str) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4, what)?.try_into().unwrap()))
    }

    pub fn i64(&mut self, what: &'static str) -> Result<i64> {
        Ok(i64::from_le_bytes(self.bytes(8, what)?.try_into().unwrap()))
    }

    /// i32 length followed by that many bytes.
    pub fn len_bytes(&mut self, what: &'static str) -> Result<&'a [u8]> {
        let n = self.i32(what)?;
        if n < 0 {
            return Err(CardError::Eof(what));
        }
        self.bytes(n as usize, what)
    }

    /// .NET string: 7-bit encoded byte length, then UTF-8.
    pub fn string(&mut self, what: &'static str) -> Result<String> {
        let mut len = 0usize;
        let mut shift = 0;
        loop {
            let b = self.u8(what)?;
            len |= ((b & 0x7f) as usize) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return Err(CardError::Eof(what));
            }
        }
        Ok(String::from_utf8_lossy(self.bytes(len, what)?).into_owned())
    }
}
