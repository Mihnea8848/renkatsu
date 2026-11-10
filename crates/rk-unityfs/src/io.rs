// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Error, Result};

/// Byte cursor with selectable endianness.
#[derive(Clone)]
pub struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
    big: bool,
}

macro_rules! num {
    ($name:ident, $t:ty, $n:expr) => {
        #[inline]
        pub fn $name(&mut self) -> Result<$t> {
            let b: [u8; $n] = self.take($n)?.try_into().unwrap();
            Ok(if self.big {
                <$t>::from_be_bytes(b)
            } else {
                <$t>::from_le_bytes(b)
            })
        }
    };
}

impl<'a> Cursor<'a> {
    pub fn new(buf: &'a [u8], big: bool) -> Self {
        Self { buf, pos: 0, big }
    }
    pub fn set_big(&mut self, big: bool) {
        self.big = big;
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }
    #[inline]
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::Eof)?;
        let s = self.buf.get(self.pos..end).ok_or(Error::Eof)?;
        self.pos = end;
        Ok(s)
    }
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
    #[inline]
    pub fn align(&mut self, n: usize) {
        self.pos = (self.pos + n - 1) & !(n - 1);
    }
    #[inline]
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    num!(u16, u16, 2);
    num!(i16, i16, 2);
    num!(u32, u32, 4);
    num!(i32, i32, 4);
    num!(u64, u64, 8);
    num!(i64, i64, 8);
    num!(f32, f32, 4);
    num!(f64, f64, 8);

    pub fn cstr(&mut self) -> Result<String> {
        let rest = self.buf.get(self.pos..).ok_or(Error::Eof)?;
        let n = rest.iter().position(|&b| b == 0).ok_or(Error::Eof)?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.pos += n + 1;
        Ok(s)
    }
}
