//! Read and write REAPER RPL preset library files: a plain-text container
//! holding one base64 blob per preset.

use crate::{err, Error, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;

/// REAPER wraps base64 at 128 characters per line.
const WRAP: usize = 128;

pub struct Rpl {
    /// Everything after `<REAPER_PRESET_LIBRARY ` on the header line,
    /// e.g. `"VST3: Archetype John Mayer X (Neural DSP)"`.
    pub header: String,
    pub presets: Vec<Preset>,
}

pub struct Preset {
    pub name: String,
    pub data: Vec<u8>,
}

pub fn parse(text: &str) -> Result<Rpl> {
    let mut header: Option<String> = None;
    let mut presets = Vec::new();
    let mut current: Option<(String, String)> = None;

    for raw in text.lines() {
        let line = raw.trim();
        let Some(_) = header else {
            if line.is_empty() {
                continue;
            }
            match line.strip_prefix("<REAPER_PRESET_LIBRARY ") {
                Some(rest) => header = Some(rest.to_string()),
                None => return err("not a REAPER preset library (missing <REAPER_PRESET_LIBRARY header)"),
            }
            continue;
        };
        if let Some(rest) = line.strip_prefix("<PRESET ") {
            current = Some((parse_quoted(rest)?, String::new()));
        } else if line == ">" {
            // Closes the current preset; the final ">" closing the library
            // arrives with no preset open and falls through.
            if let Some((name, b64)) = current.take() {
                let data = B64
                    .decode(b64.as_bytes())
                    .map_err(|e| Error(format!("invalid base64 in preset '{name}': {e}")))?;
                presets.push(Preset { name, data });
            }
        } else if let Some((_, b64)) = current.as_mut() {
            b64.push_str(line);
        }
    }

    Ok(Rpl {
        header: header.ok_or_else(|| Error("empty preset library file".into()))?,
        presets,
    })
}

fn parse_quoted(rest: &str) -> Result<String> {
    let mut chars = rest.chars();
    let quote = chars
        .next()
        .ok_or_else(|| Error("preset entry has no name".into()))?;
    if !matches!(quote, '`' | '"' | '\'') {
        // Unquoted single-word name.
        return Ok(rest.split_whitespace().next().unwrap_or(rest).to_string());
    }
    let body: String = chars.as_str().to_string();
    match body.find(quote) {
        Some(end) => Ok(body[..end].to_string()),
        None => err(format!("unterminated preset name: {rest}")),
    }
}

/// Serialize a library in REAPER's own formatting: CRLF line endings,
/// two-space indent for presets, four-space indent for base64 wrapped at
/// 128 characters.
pub fn write(header: &str, presets: &[Preset]) -> String {
    let mut out = String::new();
    out.push_str("<REAPER_PRESET_LIBRARY ");
    out.push_str(header);
    out.push_str("\r\n");
    for p in presets {
        out.push_str("  <PRESET ");
        out.push_str(&quote_name(&p.name));
        out.push_str("\r\n");
        let b64 = B64.encode(&p.data);
        let mut i = 0;
        while i < b64.len() {
            let end = (i + WRAP).min(b64.len());
            out.push_str("    ");
            out.push_str(&b64[i..end]); // base64 is ASCII, byte slicing is safe
            out.push_str("\r\n");
            i = end;
        }
        out.push_str("  >\r\n");
    }
    out.push_str(">\r\n");
    out
}

fn quote_name(name: &str) -> String {
    for q in ['`', '"', '\''] {
        if !name.contains(q) {
            return format!("{q}{name}{q}");
        }
    }
    // Name uses all three quote characters; sacrifice backticks.
    format!("`{}`", name.replace('`', "'"))
}
