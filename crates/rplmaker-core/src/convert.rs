//! High-level conversion pipeline: template RPL in, converted presets out.

use crate::blob::{Blob, StateKind};
use crate::files::CollectedFile;
use crate::rpl::{self, Preset};
use crate::valuetree::{self, Node, Value};
use crate::{err, json, treemerge, uapreset, ubjson, xmlmerge, xmlread, Error, Result};
use std::path::PathBuf;

/// The template plugin's state, in whichever form the plugin serializes it.
enum TemplateState {
    Xml(String),
    Tree(Node),
    /// Universal Audio UBJSON state, with the control keys the template's own
    /// payload carries, used to reject preset files from a different plugin.
    Ubjson {
        state: ubjson::Value,
        control_keys: Vec<String>,
    },
}

pub struct Template {
    pub library_header: String,
    pub template_preset_name: String,
    blob: Blob,
    state: TemplateState,
    state_root: String,
}

impl Template {
    /// Name of the plugin state's root node, e.g. "Euphoria" or "appModel";
    /// this is what embedded-preset scanning searches a binary for.
    pub fn state_root_name(&self) -> &str {
        &self.state_root
    }

    pub(crate) fn is_xml_state(&self) -> bool {
        matches!(self.state, TemplateState::Xml(_))
    }

    /// UA UBJSON templates have no state document embedded in the plugin
    /// binary, so binary scanning does not apply to them.
    pub(crate) fn is_ubjson_state(&self) -> bool {
        matches!(self.state, TemplateState::Ubjson { .. })
    }
}

pub struct ConvertedPreset {
    pub name: String,
    pub data: Vec<u8>,
    /// How many parameters were taken from the preset file; a sanity signal
    /// that the file really belongs to the template's plugin.
    pub parameters_applied: usize,
}

/// Load a template library exported from REAPER. Its first preset teaches
/// the converter the plugin-specific wrapper.
pub fn load_template(rpl_text: &str) -> Result<Template> {
    let lib = rpl::parse(rpl_text)?;
    let first = lib
        .presets
        .first()
        .ok_or_else(|| Error("the template RPL contains no presets".into()))?;
    let blob = Blob::parse(&first.data, &first.name)?;
    let state = match blob.kind {
        StateKind::Xml => TemplateState::Xml(
            String::from_utf8(blob.state.clone())
                .map_err(|_| Error("the template's plugin state XML is not valid UTF-8".into()))?,
        ),
        StateKind::Tree => TemplateState::Tree(valuetree::parse_with_len(&blob.state)?.0),
        StateKind::Ubjson => {
            let value = ubjson::parse(&blob.state)?.0;
            let control_keys = ua_template_control_keys(&value);
            TemplateState::Ubjson { state: value, control_keys }
        }
    };
    let state_root = match &state {
        TemplateState::Xml(xml) => xml_root_name(xml)
            .ok_or_else(|| Error("cannot find the root element of the template state".into()))?
            .to_string(),
        TemplateState::Tree(node) => node.name.clone(),
        // UBJSON states have no single root element; binary scanning does not
        // apply, so this name is only a placeholder.
        TemplateState::Ubjson { .. } => "uapw".to_string(),
    };
    Ok(Template {
        library_header: lib.header,
        template_preset_name: first.name.clone(),
        blob,
        state,
        state_root,
    })
}

/// Name of the first real element in an XML document.
fn xml_root_name(xml: &str) -> Option<&str> {
    let bytes = xml.as_bytes();
    let mut i = 0;
    while let Some(open) = xml[i..].find('<').map(|p| p + i) {
        match bytes.get(open + 1) {
            Some(b'?') | Some(b'!') => i = open + 1,
            _ => {
                let start = open + 1;
                let mut end = start;
                while end < bytes.len()
                    && !matches!(bytes[end], b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>')
                {
                    end += 1;
                }
                return Some(&xml[start..end]);
            }
        }
    }
    None
}

/// Vendor preset files come as JUCE binary ValueTrees or as XML text;
/// accept either, normalized to the same tree shape.
fn parse_preset_file(data: &[u8]) -> Result<Node> {
    let body = skip_leading_noise(data);
    if body.starts_with(b"<") {
        let text = std::str::from_utf8(body)
            .map_err(|_| Error("the preset file's XML is not valid UTF-8".into()))?;
        xmlread::parse(text)
    } else {
        valuetree::parse(data)
    }
}

/// Skip a UTF-8 byte-order mark and leading whitespace so XML detection is
/// not fooled by cosmetic bytes.
fn skip_leading_noise(data: &[u8]) -> &[u8] {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let start = data
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(data.len());
    &data[start..]
}

/// The preset's own name: vendors store it under different property names.
fn preset_display_name(vt: &Node) -> Option<String> {
    for key in ["name", "presetNameProp"] {
        if let Some(Value::Str(s)) = vt.prop(key) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Convert one vendor preset file. `fallback_name` (usually the file stem)
/// is used when the preset file carries no name of its own.
pub fn convert_preset(
    template: &Template,
    preset_file: &[u8],
    fallback_name: &str,
) -> Result<ConvertedPreset> {
    convert_with_prefix(template, preset_file, fallback_name, None)
}

/// The folder marker must be part of the name BEFORE the blob is built, so
/// the RPL entry name and the name embedded in the blob's footer stay
/// identical; a mismatch makes the output unusable as a future template.
fn convert_with_prefix(
    template: &Template,
    preset_file: &[u8],
    fallback_name: &str,
    folder_prefix: Option<&str>,
) -> Result<ConvertedPreset> {
    // Universal Audio presets are JSON, not ValueTrees, and replace the whole
    // payload rather than merging parameters, so they take their own path.
    if let TemplateState::Ubjson { state, control_keys } = &template.state {
        return convert_ua_preset(template, state, control_keys, preset_file, fallback_name, folder_prefix);
    }

    let vt = parse_preset_file(preset_file)?;
    let base_name =
        preset_display_name(&vt).unwrap_or_else(|| fallback_name.to_string());
    let name = match folder_prefix {
        Some(folder) => format!("{folder} folder: {base_name}"),
        None => base_name,
    };
    let (state, stats) = match &template.state {
        TemplateState::Xml(xml) => {
            let (merged, stats) = xmlmerge::merge(xml, &vt)?;
            (merged.into_bytes(), stats)
        }
        TemplateState::Tree(tree) => treemerge::merge(tree, &vt)?,
        TemplateState::Ubjson { .. } => unreachable!("handled above"),
    };
    if stats.overridden == 0 {
        return err(
            "no parameters matched the template; this preset file is \
             probably for a different plugin",
        );
    }
    let data = template.blob.rebuild(&state, &name);
    Ok(ConvertedPreset {
        name,
        data,
        parameters_applied: stats.overridden,
    })
}

/// The control keys the template's own payload lists, so a preset file for a
/// different UA plugin (with a different control set) can be rejected. The
/// payload is a null-terminated JSON string, so the terminator is trimmed
/// before parsing.
fn ua_template_control_keys(state: &ubjson::Value) -> Vec<String> {
    state
        .get("plugin_state_payload")
        .and_then(ubjson::Value::as_bytes)
        .and_then(|b| std::str::from_utf8(b).ok())
        .map(|text| text.trim_end_matches(['\0', ' ', '\n', '\r', '\t']))
        .and_then(|text| json::parse(text).ok())
        .map(|chunk| {
            chunk
                .get("controls")
                .map(|c| c.keys().into_iter().map(str::to_string).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

/// Convert one Universal Audio preset file by dropping its `chunk` into the
/// template's UBJSON state (both the active slot and the alternate slot), so
/// the plugin recalls the preset regardless of which slot it reads.
fn convert_ua_preset(
    template: &Template,
    template_state: &ubjson::Value,
    control_keys: &[String],
    preset_file: &[u8],
    fallback_name: &str,
    folder_prefix: Option<&str>,
) -> Result<ConvertedPreset> {
    let preset = uapreset::parse(preset_file)?;

    // Accept the file only if its controls overlap the template's; a preset
    // for a different UA plugin names different controls.
    let applied = preset
        .control_keys
        .iter()
        .filter(|k| control_keys.iter().any(|c| c == *k))
        .count();
    if applied == 0 && !control_keys.is_empty() {
        return err(
            "no controls matched the template; this UA preset file is \
             probably for a different plugin",
        );
    }

    let base_name = if preset.name.trim().is_empty() {
        fallback_name.to_string()
    } else {
        preset.name.trim().to_string()
    };
    let name = match folder_prefix {
        Some(folder) => format!("{folder} folder: {base_name}"),
        None => base_name.clone(),
    };

    let state = build_ua_state(template_state, &base_name, &preset);
    let data = template.blob.rebuild(&ubjson::write(&state), &name);
    Ok(ConvertedPreset {
        name,
        data,
        parameters_applied: preset.control_keys.len(),
    })
}

/// Clone the template state and overwrite the preset-identifying fields in
/// both the active and alternate slots with this preset's data.
fn build_ua_state(template_state: &ubjson::Value, name: &str, preset: &uapreset::UaPreset) -> ubjson::Value {
    // The plugin stores the payload as a null-terminated string; match that
    // framing when the template's payload is terminated the same way.
    let mut payload_bytes = preset.chunk_json.clone().into_bytes();
    let terminated = template_state
        .get("plugin_state_payload")
        .and_then(ubjson::Value::as_bytes)
        .and_then(<[u8]>::last)
        == Some(&0);
    if terminated {
        payload_bytes.push(0);
    }
    let payload = ubjson::Value::Bytes(payload_bytes);
    let mut state = template_state.clone();
    apply_ua_slot(&mut state, name, &preset.uid, payload.clone());
    if let Some(mut alt) = state.get("alternate_state").cloned() {
        if alt.members().is_some() {
            apply_ua_slot(&mut alt, name, &preset.uid, payload);
            state.set("alternate_state", alt);
        }
    }
    state
}

/// Set the name, uid, dirty flag and payload of one UBJSON slot object,
/// touching only members the template already has.
fn apply_ua_slot(slot: &mut ubjson::Value, name: &str, uid: &str, payload: ubjson::Value) {
    slot.set("preset_name", ubjson::Value::Str(name.to_string()));
    if !uid.is_empty() {
        slot.set("preset_uid", ubjson::Value::Str(uid.to_string()));
    }
    slot.set("preset_dirty", ubjson::Value::Bool(false));
    slot.set("plugin_state_payload", payload);
}

pub struct BatchOutcome {
    pub presets: Vec<ConvertedPreset>,
    pub skipped: Vec<(PathBuf, Error)>,
}

/// How subfolders show up in converted preset names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FolderNaming {
    /// Every preset keeps its saved name.
    Flat,
    /// The first preset of each subfolder is prefixed with the innermost
    /// folder name: "Adam Christianson folder: Dreamolo".
    Deepest,
    /// The first preset of each subfolder is prefixed with the whole
    /// relative folder path: "Artists, Adam Christianson folder: Dreamolo".
    FullPath,
}

/// Convert an ordered batch of collected files. With markers on, the first
/// preset converted from each subfolder gets "<Folder> folder: " prepended
/// to its name, so the folder change is announced while arrowing through
/// REAPER's flat preset list; every other name is left verbatim. Folder
/// identity is always the full relative path, so two same-named folders
/// under different parents each get announced even in Deepest mode.
pub fn convert_files(
    template: &Template,
    files: &[CollectedFile],
    naming: FolderNaming,
) -> BatchOutcome {
    let mut presets = Vec::new();
    let mut skipped = Vec::new();
    // The folder last announced (or entered flat). Updated only on success,
    // so if the first file of a folder fails, the marker lands on the next
    // preset that does convert from that folder.
    let mut previous_folder: Option<&[String]> = None;
    for file in files {
        let fallback = file
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Preset".to_string());
        let folder = file.subfolder.as_deref();
        let prefix: Option<String> = match folder {
            Some(components) if naming != FolderNaming::Flat && previous_folder != folder => {
                Some(match naming {
                    FolderNaming::Deepest => components
                        .last()
                        .cloned()
                        .unwrap_or_default(),
                    _ => components.join(", "),
                })
            }
            _ => None,
        };
        let result = std::fs::read(&file.path)
            .map_err(|e| Error(e.to_string()))
            .and_then(|data| convert_with_prefix(template, &data, &fallback, prefix.as_deref()));
        match result {
            Ok(preset) => {
                previous_folder = folder;
                presets.push(preset);
            }
            Err(e) => skipped.push((file.path.clone(), e)),
        }
    }
    BatchOutcome { presets, skipped }
}

/// Serialize converted presets into RPL text using the template's library
/// header, so REAPER attaches the presets to the right plugin.
pub fn build_rpl(template: &Template, presets: &[ConvertedPreset]) -> String {
    let items: Vec<Preset> = presets
        .iter()
        .map(|p| Preset { name: p.name.clone(), data: p.data.clone() })
        .collect();
    rpl::write(&template.library_header, &items)
}
