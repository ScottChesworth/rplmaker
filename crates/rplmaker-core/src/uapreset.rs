//! Universal Audio UADx preset files. Each is a small JSON document whose
//! `chunk` object is exactly what the plugin's component state carries as its
//! `plugin_state_payload` (a JSON text string), alongside the preset `name`,
//! `uid` and `plugin_id`. Converting one means dropping its `chunk` into a
//! template state, which the UBJSON layer rebuilds.

use crate::{err, json, Error, Result};

pub struct UaPreset {
    pub name: String,
    pub uid: String,
    pub plugin_id: String,
    /// The exact source text of the `chunk` value, embedded verbatim so the
    /// vendor's number formatting is preserved.
    pub chunk_json: String,
    /// The keys under `chunk.controls`, used to sanity-check that a preset
    /// belongs to the template's plugin.
    pub control_keys: Vec<String>,
}

/// Does this file look like a UA preset (a JSON object with a `chunk`)?
pub fn looks_like(data: &[u8]) -> bool {
    let text = match std::str::from_utf8(data) {
        Ok(t) => t.trim_start_matches('\u{feff}').trim_start(),
        Err(_) => return false,
    };
    text.starts_with('{') && text.contains("\"chunk\"")
}

pub fn parse(data: &[u8]) -> Result<UaPreset> {
    let text = std::str::from_utf8(data)
        .map_err(|_| Error("the UA preset file is not valid UTF-8".into()))?;
    let members = json::object_members_raw(text)?;
    let raw = |key: &str| members.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    let chunk_json = raw("chunk")
        .ok_or_else(|| Error("this UA preset file has no \"chunk\"".into()))?
        .trim()
        .to_string();

    let string_field = |key: &str| -> Result<String> {
        match raw(key) {
            Some(v) => json::parse(v)?
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| Error(format!("UA preset \"{key}\" is not a string"))),
            None => Ok(String::new()),
        }
    };
    let name = string_field("name")?;
    let uid = string_field("uid")?;
    let plugin_id = string_field("plugin_id")?;

    let chunk = json::parse(&chunk_json)?;
    let control_keys: Vec<String> = chunk
        .get("controls")
        .map(|c| c.keys().into_iter().map(str::to_string).collect())
        .unwrap_or_default();
    if control_keys.is_empty() {
        return err("this UA preset file has no controls; it is probably not a preset");
    }

    Ok(UaPreset { name, uid, plugin_id, chunk_json, control_keys })
}
