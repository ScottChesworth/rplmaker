//! Reader and writer for the UBJSON (Universal Binary JSON) container that
//! Universal Audio's UADx plugins use for their component state, in place of
//! JUCE's XML or ValueTree. The state is a UBJSON object whose
//! `plugin_state_payload` member holds the real parameters as a JSON text
//! string (stored as an int8 array).
//!
//! Only the subset UA actually emits is handled: objects (with the `#count`
//! optimization), int8-typed arrays (the payload), the five integer widths,
//! strings, floats, booleans and null. Lengths and counts are written in the
//! smallest integer type that fits, which reproduces UA's own encoding
//! byte-for-byte, so parsing and re-writing a UA state is lossless.

use crate::{err, Error, Result};

const MAX_ITEMS: i64 = 1_000_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// An integer, remembering the UBJSON type byte it was stored under so a
    /// value round-trips under its original width.
    Int { v: i64, ty: u8 },
    Float32(f32),
    Float64(f64),
    Str(String),
    /// An `[$i#..]` int8 array, kept as its raw bytes; this is how the JSON
    /// payload strings are stored.
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Members of an object value, or None for any other kind.
    pub fn members(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    /// Borrow the value stored under `key` in an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.members()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Replace (or, if absent, do nothing to) the value under `key` in an
    /// object. Returns whether a member was found and set.
    pub fn set(&mut self, key: &str, value: Value) -> bool {
        if let Value::Object(members) = self {
            if let Some(slot) = members.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
                return true;
            }
        }
        false
    }

    /// The bytes of a `Bytes` value (the raw payload), or None.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// The text of a `Str` value, or None.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Parse one UBJSON value and report how many bytes it occupied, so callers
/// can split any trailing data (the component trailer) from the value.
pub fn parse(data: &[u8]) -> Result<(Value, usize)> {
    let mut r = Reader { data, pos: 0 };
    let ty = r.byte()?;
    let value = r.value(ty, 0)?;
    Ok((value, r.pos))
}

/// Serialize a UBJSON value in UA's encoding.
pub fn write(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_value(&mut out, value);
    out
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or_else(|| Error("unexpected end of UBJSON state".into()))?;
        self.pos += 1;
        Ok(b)
    }

    fn peek(&self) -> Result<u8> {
        self.data
            .get(self.pos)
            .copied()
            .ok_or_else(|| Error("unexpected end of UBJSON state".into()))
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| Error("truncated UBJSON state".into()))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    /// Read a typed integer (the `ty` byte has already been read), used for
    /// both integer values and for lengths and counts.
    fn int(&mut self, ty: u8) -> Result<i64> {
        Ok(match ty {
            b'i' => self.take(1)?[0] as i8 as i64,
            b'U' => self.take(1)?[0] as i64,
            b'I' => i16::from_be_bytes(self.take(2)?.try_into().expect("len checked")) as i64,
            b'l' => i32::from_be_bytes(self.take(4)?.try_into().expect("len checked")) as i64,
            b'L' => i64::from_be_bytes(self.take(8)?.try_into().expect("len checked")),
            other => return err(format!("unsupported UBJSON integer type '{}'", other as char)),
        })
    }

    /// A length or count must be a non-negative integer that fits `usize`.
    fn len(&mut self) -> Result<usize> {
        let ty = self.byte()?;
        let n = self.int(ty)?;
        if !(0..MAX_ITEMS).contains(&n) {
            return err("implausible length in UBJSON state");
        }
        Ok(n as usize)
    }

    /// An object key: a length-prefixed string with no type marker.
    fn key(&mut self) -> Result<String> {
        let n = self.len()?;
        let bytes = self.take(n)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn value(&mut self, ty: u8, depth: usize) -> Result<Value> {
        if depth > 64 {
            return err("UBJSON state nests too deeply");
        }
        match ty {
            b'Z' => Ok(Value::Null),
            b'T' => Ok(Value::Bool(true)),
            b'F' => Ok(Value::Bool(false)),
            b'i' | b'U' | b'I' | b'l' | b'L' => Ok(Value::Int { v: self.int(ty)?, ty }),
            b'd' => Ok(Value::Float32(f32::from_be_bytes(
                self.take(4)?.try_into().expect("len checked"),
            ))),
            b'D' => Ok(Value::Float64(f64::from_be_bytes(
                self.take(8)?.try_into().expect("len checked"),
            ))),
            b'S' => {
                let n = self.len()?;
                let bytes = self.take(n)?;
                Ok(Value::Str(String::from_utf8_lossy(bytes).into_owned()))
            }
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            other => err(format!("unsupported UBJSON type '{}'", other as char)),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value> {
        let mut members = Vec::new();
        if self.peek()? == b'#' {
            self.pos += 1;
            let count = self.len()?;
            for _ in 0..count {
                let key = self.key()?;
                let ty = self.byte()?;
                members.push((key, self.value(ty, depth + 1)?));
            }
        } else {
            while self.peek()? != b'}' {
                let key = self.key()?;
                let ty = self.byte()?;
                members.push((key, self.value(ty, depth + 1)?));
            }
            self.pos += 1; // closing '}'
        }
        Ok(Value::Object(members))
    }

    fn array(&mut self, depth: usize) -> Result<Value> {
        let mut elem_ty = None;
        if self.peek()? == b'$' {
            self.pos += 1;
            elem_ty = Some(self.byte()?);
        }
        let count = if self.peek()? == b'#' {
            self.pos += 1;
            Some(self.len()?)
        } else {
            None
        };
        // The int8 array used for the JSON payload: keep it as raw bytes.
        if elem_ty == Some(b'i') {
            let n = count.ok_or_else(|| Error("typed UBJSON array without a count".into()))?;
            return Ok(Value::Bytes(self.take(n)?.to_vec()));
        }
        let mut items = Vec::new();
        match count {
            Some(n) => {
                for _ in 0..n {
                    let ty = match elem_ty {
                        Some(t) => t,
                        None => self.byte()?,
                    };
                    items.push(self.value(ty, depth + 1)?);
                }
            }
            None => {
                while self.peek()? != b']' {
                    let ty = self.byte()?;
                    items.push(self.value(ty, depth + 1)?);
                }
                self.pos += 1; // closing ']'
            }
        }
        Ok(Value::Array(items))
    }
}

/// Append the smallest integer type and value that encodes a non-negative
/// count or length, matching UA's encoding.
fn write_len(out: &mut Vec<u8>, n: usize) {
    if n <= i8::MAX as usize {
        out.push(b'i');
        out.push(n as u8);
    } else if n <= u8::MAX as usize {
        out.push(b'U');
        out.push(n as u8);
    } else if n <= i16::MAX as usize {
        out.push(b'I');
        out.extend_from_slice(&(n as i16).to_be_bytes());
    } else if n <= i32::MAX as usize {
        out.push(b'l');
        out.extend_from_slice(&(n as i32).to_be_bytes());
    } else {
        out.push(b'L');
        out.extend_from_slice(&(n as i64).to_be_bytes());
    }
}

fn write_int(out: &mut Vec<u8>, v: i64, ty: u8) {
    out.push(ty);
    match ty {
        b'i' => out.push(v as i8 as u8),
        b'U' => out.push(v as u8),
        b'I' => out.extend_from_slice(&(v as i16).to_be_bytes()),
        b'l' => out.extend_from_slice(&(v as i32).to_be_bytes()),
        _ => out.extend_from_slice(&v.to_be_bytes()),
    }
}

fn write_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.push(b'Z'),
        Value::Bool(true) => out.push(b'T'),
        Value::Bool(false) => out.push(b'F'),
        Value::Int { v, ty } => write_int(out, *v, *ty),
        Value::Float32(f) => {
            out.push(b'd');
            out.extend_from_slice(&f.to_be_bytes());
        }
        Value::Float64(f) => {
            out.push(b'D');
            out.extend_from_slice(&f.to_be_bytes());
        }
        Value::Str(s) => {
            out.push(b'S');
            write_len(out, s.len());
            out.extend_from_slice(s.as_bytes());
        }
        Value::Bytes(b) => {
            out.extend_from_slice(b"[$i#");
            write_len(out, b.len());
            out.extend_from_slice(b);
        }
        Value::Array(items) => {
            out.push(b'[');
            out.push(b'#');
            write_len(out, items.len());
            for item in items {
                write_value(out, item);
            }
        }
        Value::Object(members) => {
            out.push(b'{');
            out.push(b'#');
            write_len(out, members.len());
            for (key, val) in members {
                write_len(out, key.len());
                out.extend_from_slice(key.as_bytes());
                write_value(out, val);
            }
        }
    }
}
