//! Reader and writer for JUCE binary ValueTree data. Many plugin vendors
//! (Neural DSP among them) save presets in this format, sometimes behind a
//! misleading ".xml" extension; older JUCE plugins also use it for their
//! VST3 state itself.
//!
//! Stream layout per node: null-terminated name, compressed-int property
//! count, properties (name + var), compressed-int child count, children.
//! A var is a compressed-int payload size followed by a one-byte type code.

use crate::{err, Error, Result};

const MAX_ITEMS: i64 = 100_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Int64(i64),
    Bool(bool),
    Double(f64),
    Str(String),
    /// A zero-size var (JUCE void).
    Void,
    /// A var type the converter has no use for (arrays, binary), kept as
    /// its raw type code and payload so it round-trips byte-identically.
    Other(u8, Vec<u8>),
}

impl Value {
    /// Text form as JUCE would write it into an XML attribute.
    pub fn as_text(&self) -> String {
        match self {
            Value::Int(v) => v.to_string(),
            Value::Int64(v) => v.to_string(),
            Value::Bool(true) => "true".into(),
            Value::Bool(false) => "false".into(),
            Value::Double(v) => v.to_string(),
            Value::Str(s) => s.clone(),
            Value::Void | Value::Other(..) => String::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Node {
    pub name: String,
    pub props: Vec<(String, Value)>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn prop(&self, name: &str) -> Option<&Value> {
        self.props.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
}

/// Depth-first search for the first node with the given name.
pub fn find_node<'a>(root: &'a Node, name: &str) -> Option<&'a Node> {
    if root.name == name {
        return Some(root);
    }
    root.children.iter().find_map(|c| find_node(c, name))
}

pub fn parse(data: &[u8]) -> Result<Node> {
    if data.starts_with(b"<") {
        return err(
            "this file is XML text, not a JUCE binary preset; \
             it must be parsed with the XML reader instead",
        );
    }
    parse_with_len(data).map(|(node, _)| node)
}

/// Parse one tree and also report how many bytes it occupied, so callers
/// can split trailing data (e.g. the JUCEPrivateData block after a plugin
/// state) from the tree itself.
pub fn parse_with_len(data: &[u8]) -> Result<(Node, usize)> {
    let mut r = Reader { data, pos: 0 };
    let node = r.read_node(0)?;
    Ok((node, r.pos))
}

/// Serialize a tree in JUCE's binary format. Parsing and re-serializing a
/// JUCE-produced stream reproduces it byte for byte.
pub fn write(node: &Node) -> Vec<u8> {
    let mut out = Vec::new();
    write_node(&mut out, node);
    out
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| Error("unexpected end of preset file".into()))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn read_string(&mut self) -> Result<String> {
        let start = self.pos;
        while self.pos < self.data.len() && self.data[self.pos] != 0 {
            self.pos += 1;
        }
        if self.pos >= self.data.len() {
            return err("unterminated string in preset file");
        }
        let s = String::from_utf8_lossy(&self.data[start..self.pos]).into_owned();
        self.pos += 1; // null terminator
        Ok(s)
    }

    /// JUCE OutputStream::writeCompressedInt: a length byte (sign in the top
    /// bit) followed by that many little-endian magnitude bytes.
    fn read_compressed_int(&mut self) -> Result<i64> {
        let head = self.take(1)?[0];
        let negative = head & 0x80 != 0;
        let num_bytes = (head & 0x7f) as usize;
        if num_bytes > 8 {
            return err("corrupt compressed integer in preset file");
        }
        let mut v: i64 = 0;
        for (i, b) in self.take(num_bytes)?.iter().enumerate() {
            v |= (*b as i64) << (8 * i);
        }
        Ok(if negative { -v } else { v })
    }

    fn read_var(&mut self) -> Result<Value> {
        let size = self.read_compressed_int()?;
        if size == 0 {
            return Ok(Value::Void);
        }
        if size < 0 {
            return err("corrupt property size in preset file");
        }
        let ty = self.take(1)?[0];
        let body = self.take(size as usize - 1)?;
        Ok(match (ty, body.len()) {
            (1, 4) => Value::Int(i32::from_le_bytes(body.try_into().expect("len checked"))),
            (2, 0) => Value::Bool(true),
            (3, 0) => Value::Bool(false),
            (4, 8) => Value::Double(f64::from_le_bytes(body.try_into().expect("len checked"))),
            (5, _) if body.last() == Some(&0) => {
                Value::Str(String::from_utf8_lossy(&body[..body.len() - 1]).into_owned())
            }
            (6, 8) => Value::Int64(i64::from_le_bytes(body.try_into().expect("len checked"))),
            _ => Value::Other(ty, body.to_vec()),
        })
    }

    fn read_node(&mut self, depth: usize) -> Result<Node> {
        if depth > 64 {
            return err("preset file nests too deeply");
        }
        let name = self.read_string()?;
        if name.is_empty() {
            return err("empty node name; not a JUCE binary preset file");
        }
        let num_props = self.read_compressed_int()?;
        if !(0..MAX_ITEMS).contains(&num_props) {
            return err("implausible property count; not a JUCE binary preset file");
        }
        let mut props = Vec::with_capacity(num_props as usize);
        for _ in 0..num_props {
            let pname = self.read_string()?;
            let value = self.read_var()?;
            props.push((pname, value));
        }
        let num_children = self.read_compressed_int()?;
        if !(0..MAX_ITEMS).contains(&num_children) {
            return err("implausible child count; not a JUCE binary preset file");
        }
        let mut children = Vec::with_capacity(num_children as usize);
        for _ in 0..num_children {
            children.push(self.read_node(depth + 1)?);
        }
        Ok(Node { name, props, children })
    }
}

fn write_node(out: &mut Vec<u8>, node: &Node) {
    write_string(out, &node.name);
    write_compressed_uint(out, node.props.len() as u64);
    for (name, value) in &node.props {
        write_string(out, name);
        write_var(out, value);
    }
    write_compressed_uint(out, node.children.len() as u64);
    for child in &node.children {
        write_node(out, child);
    }
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

fn write_compressed_uint(out: &mut Vec<u8>, v: u64) {
    if v == 0 {
        out.push(0);
        return;
    }
    let bytes = v.to_le_bytes();
    let len = 8 - bytes.iter().rev().take_while(|&&b| b == 0).count();
    out.push(len as u8);
    out.extend_from_slice(&bytes[..len]);
}

fn write_var(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Void => write_compressed_uint(out, 0),
        Value::Int(v) => {
            write_compressed_uint(out, 5);
            out.push(1);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Bool(true) => {
            write_compressed_uint(out, 1);
            out.push(2);
        }
        Value::Bool(false) => {
            write_compressed_uint(out, 1);
            out.push(3);
        }
        Value::Double(v) => {
            write_compressed_uint(out, 9);
            out.push(4);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Str(s) => {
            write_compressed_uint(out, s.len() as u64 + 2);
            out.push(5);
            write_string(out, s);
        }
        Value::Int64(v) => {
            write_compressed_uint(out, 9);
            out.push(6);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Other(ty, body) => {
            write_compressed_uint(out, body.len() as u64 + 1);
            out.push(*ty);
            out.extend_from_slice(body);
        }
    }
}
