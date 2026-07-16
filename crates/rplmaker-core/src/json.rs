//! A small JSON reader, just enough to read Universal Audio's preset files.
//! Two entry points: `parse` for a full value tree (used to inspect a
//! preset's control set), and `object_members_raw`, which returns a
//! top-level object's members paired with the exact source text of each
//! value. The raw text lets the converter embed a preset's `chunk` verbatim,
//! preserving the vendor's exact number formatting rather than round-tripping
//! it through a float.

use crate::{err, Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Keys of an object, in order; empty for any other kind.
    pub fn keys(&self) -> Vec<&str> {
        match self {
            Json::Object(m) => m.iter().map(|(k, _)| k.as_str()).collect(),
            _ => Vec::new(),
        }
    }
}

/// Parse a complete JSON document.
pub fn parse(text: &str) -> Result<Json> {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return err("trailing data after JSON document");
    }
    Ok(v)
}

/// Parse a top-level JSON object and return its members as (key, raw value
/// text) pairs, where the raw text is the exact source slice of each value.
pub fn object_members_raw(text: &str) -> Result<Vec<(String, String)>> {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    p.ws();
    if p.byte()? != b'{' {
        return err("expected a JSON object");
    }
    let mut members = Vec::new();
    p.ws();
    if p.peek()? == b'}' {
        return Ok(members);
    }
    loop {
        p.ws();
        let key = p.string()?;
        p.ws();
        if p.byte()? != b':' {
            return err("expected ':' in JSON object");
        }
        p.ws();
        let start = p.i;
        p.skip_value(0)?;
        let raw = std::str::from_utf8(&p.s[start..p.i])
            .map_err(|_| Error("non-UTF-8 JSON value".into()))?
            .to_string();
        members.push((key, raw));
        p.ws();
        match p.byte()? {
            b',' => continue,
            b'}' => break,
            _ => return err("expected ',' or '}' in JSON object"),
        }
    }
    Ok(members)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self.s.get(self.i).ok_or_else(|| Error("unexpected end of JSON".into()))?;
        self.i += 1;
        Ok(b)
    }

    fn peek(&self) -> Result<u8> {
        self.s.get(self.i).copied().ok_or_else(|| Error("unexpected end of JSON".into()))
    }

    fn ws(&mut self) {
        while let Some(&b) = self.s.get(self.i) {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json> {
        if depth > 64 {
            return err("JSON nests too deeply");
        }
        match self.peek()? {
            b'{' => {
                self.i += 1;
                let mut members = Vec::new();
                self.ws();
                if self.peek()? == b'}' {
                    self.i += 1;
                    return Ok(Json::Object(members));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    self.ws();
                    if self.byte()? != b':' {
                        return err("expected ':' in JSON object");
                    }
                    self.ws();
                    members.push((key, self.value(depth + 1)?));
                    self.ws();
                    match self.byte()? {
                        b',' => continue,
                        b'}' => break,
                        _ => return err("expected ',' or '}' in JSON object"),
                    }
                }
                Ok(Json::Object(members))
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek()? == b']' {
                    self.i += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.byte()? {
                        b',' => continue,
                        b']' => break,
                        _ => return err("expected ',' or ']' in JSON array"),
                    }
                }
                Ok(Json::Array(items))
            }
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => {
                self.literal("true")?;
                Ok(Json::Bool(true))
            }
            b'f' => {
                self.literal("false")?;
                Ok(Json::Bool(false))
            }
            b'n' => {
                self.literal("null")?;
                Ok(Json::Null)
            }
            _ => self.number(),
        }
    }

    /// Advance past a value without building it, for span capture.
    fn skip_value(&mut self, depth: usize) -> Result<()> {
        self.value(depth).map(|_| ())
    }

    fn literal(&mut self, word: &str) -> Result<()> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(())
        } else {
            err(format!("expected '{word}' in JSON"))
        }
    }

    fn number(&mut self) -> Result<Json> {
        let start = self.i;
        while let Some(&b) = self.s.get(self.i) {
            if matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i == start {
            return err("expected a JSON value");
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()
            .and_then(|t| t.parse::<f64>().ok())
            .map(Json::Num)
            .ok_or_else(|| Error("invalid JSON number".into()))
    }

    fn string(&mut self) -> Result<String> {
        if self.byte()? != b'"' {
            return err("expected a JSON string");
        }
        let mut out = String::new();
        loop {
            match self.byte()? {
                b'"' => return Ok(out),
                b'\\' => match self.byte()? {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = self.take(4)?;
                        let code = u32::from_str_radix(
                            std::str::from_utf8(hex).map_err(|_| Error("bad \\u escape".into()))?,
                            16,
                        )
                        .map_err(|_| Error("bad \\u escape".into()))?;
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    other => return err(format!("invalid JSON escape '\\{}'", other as char)),
                },
                b => {
                    // Bytes >= 0x80 are UTF-8 continuation; collect the run and
                    // decode lossily so multibyte characters survive.
                    if b < 0x80 {
                        out.push(b as char);
                    } else {
                        let start = self.i - 1;
                        while self.s.get(self.i).is_some_and(|&c| c >= 0x80) {
                            self.i += 1;
                        }
                        out.push_str(&String::from_utf8_lossy(&self.s[start..self.i]));
                    }
                }
            }
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .i
            .checked_add(n)
            .filter(|&e| e <= self.s.len())
            .ok_or_else(|| Error("unexpected end of JSON".into()))?;
        let s = &self.s[self.i..end];
        self.i = end;
        Ok(s)
    }
}
