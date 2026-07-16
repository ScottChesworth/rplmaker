//! Overlay parameter values from a parsed preset ValueTree onto the
//! template's plugin-state XML.
//!
//! The template XML is walked element by element; each element is matched to
//! the ValueTree node with the same name at the same tree position (JUCE
//! serializes the identical tree to both formats, so names line up
//! one-to-one). Attributes whose name exists as a property on the matched
//! node get the preset's value; everything else — attribute order, spacing,
//! attributes only REAPER's capture includes, whole elements missing from
//! the preset file — is preserved byte-for-byte from the template. This is
//! deliberately a scanner for JUCE's single-line machine-generated XML, not
//! a general XML parser.

use crate::valuetree::{find_node, Node, Value};
use crate::{err, Error, Result};
use std::collections::HashMap;

pub struct MergeStats {
    /// Attributes replaced with values from the preset file.
    pub overridden: usize,
    /// Attributes kept from the template (no matching property).
    pub template_only: usize,
}

/// One open element: its matched source node plus how many same-named
/// children have been seen, so repeated elements (JUCE's PARAM nodes) can
/// be matched by their "id" attribute or by ordinal, never collapsed onto
/// the first namesake.
struct Frame<'a> {
    vt: Option<&'a Node>,
    seen: HashMap<String, usize>,
}

/// The template element's "id" attribute, if any, scanned ahead of the
/// main attribute loop. `from` points just past the element name.
fn tag_id_attribute(template_xml: &str, from: usize) -> Option<&str> {
    let bytes = template_xml.as_bytes();
    let mut i = from;
    while i < bytes.len() && bytes[i] != b'>' {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
        } else if template_xml[i..].starts_with("id=") && bytes[i - 1].is_ascii_whitespace() {
            let quote = *bytes.get(i + 3)?;
            let start = i + 4;
            let len = template_xml[start..].find(quote as char)?;
            return Some(&template_xml[start..start + len]);
        }
        i += 1;
    }
    None
}

fn match_child<'a>(
    parent: &'a Node,
    name: &str,
    id: Option<&str>,
    ordinal: usize,
) -> Option<&'a Node> {
    let same_named = parent.children.iter().filter(|c| c.name == name);
    match id {
        Some(id) => {
            let mut same_named = same_named;
            same_named.find(|c| c.prop("id").map(Value::as_text).as_deref() == Some(id))
        }
        None => same_named.into_iter().nth(ordinal),
    }
}

pub fn merge(template_xml: &str, vt_root: &Node) -> Result<(String, MergeStats)> {
    let b = template_xml.as_bytes();
    let mut out = String::with_capacity(template_xml.len() + 64);
    let mut stack: Vec<Frame> = Vec::new();
    let mut stats = MergeStats { overridden: 0, template_only: 0 };
    let mut i = 0;

    while i < b.len() {
        if b[i] != b'<' {
            let start = i;
            while i < b.len() && b[i] != b'<' {
                i += 1;
            }
            out.push_str(&template_xml[start..i]);
            continue;
        }
        if template_xml[i..].starts_with("<?") {
            let end = template_xml[i..]
                .find("?>")
                .ok_or_else(|| Error("unterminated XML declaration in template".into()))?
                + i
                + 2;
            out.push_str(&template_xml[i..end]);
            i = end;
            continue;
        }
        if template_xml[i..].starts_with("</") {
            let end = template_xml[i..]
                .find('>')
                .ok_or_else(|| Error("unterminated closing tag in template XML".into()))?
                + i
                + 1;
            out.push_str(&template_xml[i..end]);
            stack.pop();
            i = end;
            continue;
        }

        // Start tag: element name, then attributes until '>' or '/>'.
        let name_start = i + 1;
        let mut j = name_start;
        while j < b.len() && !matches!(b[j], b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>') {
            j += 1;
        }
        let name = &template_xml[name_start..j];
        let id = tag_id_attribute(template_xml, j);
        let vt: Option<&Node> = match stack.last_mut() {
            // The XML root sits somewhere inside the preset file's tree
            // (e.g. appModel under the vendor's root node), so search for it.
            None => find_node(vt_root, name),
            Some(frame) => {
                let ordinal = frame.seen.entry(name.to_string()).or_insert(0);
                let matched = frame
                    .vt
                    .and_then(|parent| match_child(parent, name, id, *ordinal));
                *ordinal += 1;
                matched
            }
        };
        out.push_str(&template_xml[i..j]);
        i = j;

        loop {
            let ws_start = i;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            out.push_str(&template_xml[ws_start..i]);
            if i >= b.len() {
                return err("unterminated tag in template XML");
            }
            if b[i] == b'/' {
                let end = template_xml[i..]
                    .find('>')
                    .ok_or_else(|| Error("unterminated tag in template XML".into()))?
                    + i
                    + 1;
                out.push_str(&template_xml[i..end]); // self-closing: no stack push
                i = end;
                break;
            }
            if b[i] == b'>' {
                out.push('>');
                i += 1;
                stack.push(Frame { vt, seen: HashMap::new() });
                break;
            }

            let attr_start = i;
            while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            let attr = &template_xml[attr_start..i];
            if i >= b.len() || b[i] != b'=' {
                return err(format!("malformed attribute '{attr}' in template XML"));
            }
            i += 1;
            if i >= b.len() || !matches!(b[i], b'"' | b'\'') {
                return err(format!("unquoted value for attribute '{attr}' in template XML"));
            }
            let quote = b[i];
            i += 1;
            let value_start = i;
            while i < b.len() && b[i] != quote {
                i += 1;
            }
            if i >= b.len() {
                return err(format!("unterminated value for attribute '{attr}' in template XML"));
            }
            let template_value = &template_xml[value_start..i];
            i += 1;

            out.push_str(attr);
            out.push('=');
            out.push(quote as char);
            match vt.and_then(|n| n.prop(attr)) {
                Some(value) => {
                    out.push_str(&escape_attr(&value.as_text()));
                    stats.overridden += 1;
                }
                None => {
                    out.push_str(template_value);
                    stats.template_only += 1;
                }
            }
            out.push(quote as char);
        }
    }

    Ok((out, stats))
}

fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
