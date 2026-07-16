//! Read JUCE-style XML text into the same Node shape the binary ValueTree
//! parser produces, so both vendor preset formats feed one merge path.
//! Attribute values become Value::Str; element text content is ignored
//! (JUCE presets are attribute-based). Deliberately a scanner for
//! machine-generated XML, like xmlmerge, not a general XML parser.

use crate::valuetree::{Node, Value};
use crate::{err, Error, Result};

pub fn parse(text: &str) -> Result<Node> {
    let b = text.as_bytes();
    let mut i = 0;
    // Nodes still being built; the finished root ends up alone in `roots`.
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;

    while i < b.len() {
        if b[i] != b'<' {
            i += 1; // text content and whitespace between tags
            continue;
        }
        if text[i..].starts_with("<?") {
            i = skip_past(text, i, "?>")?;
            continue;
        }
        if text[i..].starts_with("<!--") {
            i = skip_past(text, i, "-->")?;
            continue;
        }
        if text[i..].starts_with("<!") {
            i = skip_past(text, i, ">")?; // DOCTYPE and friends
            continue;
        }
        if text[i..].starts_with("</") {
            i = skip_past(text, i, ">")?;
            let finished = stack
                .pop()
                .ok_or_else(|| Error("closing tag without an open element".into()))?;
            attach(finished, &mut stack, &mut root)?;
            continue;
        }

        // Start tag.
        let name_start = i + 1;
        let mut j = name_start;
        while j < b.len() && !matches!(b[j], b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>') {
            j += 1;
        }
        let mut node = Node {
            name: text[name_start..j].to_string(),
            props: Vec::new(),
            children: Vec::new(),
        };
        i = j;

        loop {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= b.len() {
                return err("unterminated tag in preset XML");
            }
            if b[i] == b'/' {
                i = skip_past(text, i, ">")?;
                attach(node, &mut stack, &mut root)?; // self-closing
                break;
            }
            if b[i] == b'>' {
                i += 1;
                stack.push(node);
                break;
            }

            let attr_start = i;
            while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            let attr = text[attr_start..i].to_string();
            if i >= b.len() || b[i] != b'=' {
                return err(format!("malformed attribute '{attr}' in preset XML"));
            }
            i += 1;
            if i >= b.len() || !matches!(b[i], b'"' | b'\'') {
                return err(format!("unquoted value for attribute '{attr}' in preset XML"));
            }
            let quote = b[i];
            i += 1;
            let value_start = i;
            while i < b.len() && b[i] != quote {
                i += 1;
            }
            if i >= b.len() {
                return err(format!("unterminated value for attribute '{attr}' in preset XML"));
            }
            node.props
                .push((attr, Value::Str(unescape(&text[value_start..i]))));
            i += 1;
        }
    }

    if !stack.is_empty() {
        return err("preset XML ended with unclosed elements");
    }
    root.ok_or_else(|| Error("preset XML contains no elements".into()))
}

fn attach(node: Node, stack: &mut Vec<Node>, root: &mut Option<Node>) -> Result<()> {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None if root.is_none() => *root = Some(node),
        None => return err("preset XML has more than one root element"),
    }
    Ok(())
}

fn skip_past(text: &str, from: usize, marker: &str) -> Result<usize> {
    text[from..]
        .find(marker)
        .map(|p| from + p + marker.len())
        .ok_or_else(|| Error(format!("unterminated '{marker}' section in preset XML")))
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let end = match rest.find(';') {
            Some(e) => e,
            None => break, // stray ampersand; keep verbatim
        };
        let entity = &rest[1..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let parsed = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32);
                match parsed {
                    Some(c) => out.push(c),
                    None => out.push_str(&rest[..end + 1]), // unknown entity, keep verbatim
                }
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}
