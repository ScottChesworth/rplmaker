//! Scan a plugin binary for embedded factory presets.
//!
//! Many JUCE plugins compile their factory presets into the plugin file
//! itself, so no preset files exist on disk. The template tells us the
//! state's root node name; anything in the binary shaped like a complete
//! state document with that root is a preset candidate. Truncated documents
//! are skipped and duplicates removed. Preset names are not recoverable
//! this way — vendors store them in undocumented metadata — so callers
//! assign positional names.

use crate::convert::Template;
use crate::valuetree;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

/// Extract candidate preset documents (XML text or binary ValueTrees,
/// matching the template's state form) from a plugin binary.
pub fn extract_embedded_presets(template: &Template, binary: &[u8]) -> Vec<Vec<u8>> {
    if template.is_ubjson_state() {
        // UA plugins deliver factory presets as JSON files and their service,
        // not as state documents embedded in the binary; nothing to scan.
        Vec::new()
    } else if template.is_xml_state() {
        scan_xml_documents(binary, template.state_root_name())
    } else {
        scan_value_trees(binary, template.state_root_name())
    }
}

fn scan_xml_documents(data: &[u8], root: &str) -> Vec<Vec<u8>> {
    let open = format!("<{root} ").into_bytes();
    let close = format!("</{root}>").into_bytes();
    let mut docs = Vec::new();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut pos = 0;
    while let Some(start) = find(&data[pos..], &open).map(|p| p + pos) {
        pos = start + 1;
        let Some(end) = find(&data[start..], &close).map(|p| p + start + close.len()) else {
            break;
        };
        let doc = &data[start..end];
        // A second opening tag inside means the first document was
        // truncated in the binary and we spanned into the next one.
        if find(&doc[1..], &open).is_some() {
            continue;
        }
        if seen.insert(doc.to_vec()) {
            docs.push(doc.to_vec());
        }
    }
    docs
}

fn scan_value_trees(data: &[u8], root: &str) -> Vec<Vec<u8>> {
    let mut pattern = root.as_bytes().to_vec();
    pattern.push(0);
    let mut docs = Vec::new();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut pos = 0;
    while let Some(start) = find(&data[pos..], &pattern).map(|p| p + pos) {
        pos = start + 1;
        if let Ok((node, len)) = valuetree::parse_with_len(&data[start..]) {
            // Guard against random bytes that scan as a tiny tree.
            if len >= 64 && (!node.props.is_empty() || !node.children.is_empty()) {
                let doc = &data[start..start + len];
                if seen.insert(doc.to_vec()) {
                    docs.push(doc.to_vec());
                }
                pos = start + len;
            }
        }
    }
    docs
}

/// Write extracted documents as preset files the normal conversion flow can
/// pick up, named positionally to preserve the binary's storage order. The
/// directory is cleared first so re-scanning never mixes stale results in.
pub fn write_extracted(
    dir: &Path,
    stem: &str,
    docs: &[Vec<u8>],
) -> io::Result<Vec<PathBuf>> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let width = docs.len().to_string().len().max(3);
    let mut paths = Vec::with_capacity(docs.len());
    for (i, doc) in docs.iter().enumerate() {
        let path = dir.join(format!("{stem} {:0width$}.xml", i + 1));
        std::fs::write(&path, doc)?;
        paths.push(path);
    }
    Ok(paths)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
