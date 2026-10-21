//! Compound text encoder and decoder supporting UTF-8, Chinese GB2312, Japanese and Korean.

#![no_std]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

#[cfg(feature = "std")]
use std::io::{self, Write};

const UTF8_START: &[u8] = &[0x1B, 0x25, 0x47];
const UTF8_END: &[u8] = &[0x1B, 0x25, 0x40];

/// Wrapper to reduce allocation.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CText<'s> {
    utf8: &'s str,
}

impl<'s> fmt::Debug for CText<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.utf8)
    }
}

impl<'s> fmt::Display for CText<'s> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.utf8)
    }
}

impl<'s> CText<'s> {
    pub const fn new(utf8: &'s str) -> Self {
        Self { utf8 }
    }

    pub const fn len(self) -> usize {
        self.utf8.len() + UTF8_START.len() + UTF8_END.len()
    }

    pub const fn is_empty(self) -> bool {
        self.utf8.is_empty()
    }

    #[cfg(feature = "std")]
    pub fn write(self, mut out: impl Write) -> io::Result<usize> {
        let mut writed = 0;
        writed += out.write(UTF8_START)?;
        writed += out.write(self.utf8.as_bytes())?;
        writed += out.write(UTF8_END)?;
        Ok(writed)
    }
}

/// Encodes UTF-8 to compound text.
pub fn utf8_to_compound_text(text: &str) -> Vec<u8> {
    let mut ret = Vec::with_capacity(text.len() + 6);
    ret.extend_from_slice(UTF8_START);
    ret.extend_from_slice(text.as_bytes());
    ret.extend_from_slice(UTF8_END);
    ret
}

#[derive(Debug, Clone)]
pub enum DecodeError {
    InvalidEncoding,
    UnsupportedEncoding,
    Utf8Error(alloc::string::FromUtf8Error),
}

impl From<alloc::string::FromUtf8Error> for DecodeError {
    fn from(err: alloc::string::FromUtf8Error) -> Self {
        DecodeError::Utf8Error(err)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEncoding => write!(f, "Invalid compound text"),
            Self::UnsupportedEncoding => write!(f, "This encoding is not supported yet"),
            Self::Utf8Error(e) => write!(f, "Not a valid utf8 {e}"),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum State {
    Ascii,
    Utf8,
    Gb2312,
    Jis0208,
    Ks1001,
}

fn flush(state: State, buf: &mut Vec<u8>, out: &mut String) {
    if buf.is_empty() {
        return;
    }
    match state {
        State::Ascii | State::Utf8 => {
            if let Ok(s) = core::str::from_utf8(buf) {
                out.push_str(s);
            } else {
                let (cow, _, _) = encoding_rs::GB18030.decode(buf);
                out.push_str(&cow);
            }
        }
        State::Gb2312 => {
            let (cow, _, _) = encoding_rs::GB18030.decode(buf);
            out.push_str(&cow);
        }
        State::Jis0208 => {
            let mut prefix = Vec::with_capacity(3 + buf.len());
            prefix.extend_from_slice(&[0x1B, 0x24, 0x42]);
            prefix.extend_from_slice(buf);
            let (cow, _, _) = encoding_rs::ISO_2022_JP.decode(&prefix);
            out.push_str(&cow);
        }
        State::Ks1001 => {
            let (cow, _, _) = encoding_rs::EUC_KR.decode(buf);
            out.push_str(&cow);
        }
    }
    buf.clear();
}

/// Decodes compound text bytes to UTF-8 without failing on Chinese or Japanese.
pub fn compound_text_to_utf8(bytes: &[u8]) -> Result<String, DecodeError> {
    if bytes.is_empty() {
        return Ok(String::new());
    }
    if !bytes.contains(&0x1B) {
        if let Ok(s) = core::str::from_utf8(bytes) {
            return Ok(String::from(s));
        }
    }
    let mut out = String::new();
    let mut i = 0;
    let mut state = State::Ascii;
    let mut buf = Vec::new();

    while i < bytes.len() {
        if bytes[i] == 0x1B {
            let rest = &bytes[i..];
            if rest.starts_with(UTF8_START) {
                flush(state, &mut buf, &mut out);
                state = State::Utf8;
                i += 3;
                continue;
            } else if rest.starts_with(UTF8_END) {
                flush(state, &mut buf, &mut out);
                state = State::Ascii;
                i += 3;
                continue;
            } else if rest.starts_with(&[0x1B, 0x24, 0x28, 0x41]) {
                flush(state, &mut buf, &mut out);
                state = State::Gb2312;
                i += 4;
                continue;
            } else if rest.starts_with(&[0x1B, 0x24, 0x41]) {
                flush(state, &mut buf, &mut out);
                state = State::Gb2312;
                i += 3;
                continue;
            } else if rest.starts_with(&[0x1B, 0x24, 0x28, 0x42]) {
                flush(state, &mut buf, &mut out);
                state = State::Jis0208;
                i += 4;
                continue;
            } else if rest.starts_with(&[0x1B, 0x24, 0x42]) {
                flush(state, &mut buf, &mut out);
                state = State::Jis0208;
                i += 3;
                continue;
            } else if rest.starts_with(&[0x1B, 0x24, 0x28, 0x43]) {
                flush(state, &mut buf, &mut out);
                state = State::Ks1001;
                i += 4;
                continue;
            } else if rest.starts_with(&[0x1B, 0x24, 0x43]) {
                flush(state, &mut buf, &mut out);
                state = State::Ks1001;
                i += 3;
                continue;
            } else if rest.starts_with(&[0x1B, 0x28, 0x42]) || rest.starts_with(&[0x1B, 0x28, 0x4A]) {
                flush(state, &mut buf, &mut out);
                state = State::Ascii;
                i += 3;
                continue;
            }
        }
        let b = bytes[i];
        match state {
            State::Ascii | State::Utf8 | State::Jis0208 => buf.push(b),
            State::Gb2312 | State::Ks1001 => buf.push(b | 0x80),
        }
        i += 1;
    }
    flush(state, &mut buf, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn korean() {
        const UTF8: &str = "가나다";
        const COMP: &[u8] = &[
            27, 37, 71, 234, 176, 128, 235, 130, 152, 235, 139, 164, 27, 37, 64,
        ];
        assert_eq!(utf8_to_compound_text(UTF8), COMP);
        assert_eq!(compound_text_to_utf8(COMP).unwrap(), UTF8);
    }

    #[test]
    fn iso_2022_jp() {
        const UTF8: &str = "東京";
        const COMP: &[u8] = &[27, 36, 40, 66, 69, 108, 53, 126];
        assert_eq!(compound_text_to_utf8(COMP).unwrap(), UTF8);
    }

    #[test]
    fn chinese_gb2312() {
        const UTF8: &str = "你好";
        const COMP: &[u8] = &[27, 36, 40, 65, 68, 99, 58, 67];
        assert_eq!(compound_text_to_utf8(COMP).unwrap(), UTF8);
    }
}
