//! Integration tests against the real Neural DSP example files in
//! Examples/Neural DSP: three vendor preset files plus the same three
//! presets hand-saved through REAPER into an RPL. The RPL doubles as the
//! conversion template and as ground truth for what REAPER itself produces.

use rplmaker_core::{
    build_rpl, convert_files, convert_preset, load_template, rpl, valuetree, FolderNaming,
};
use std::fs;
use std::path::PathBuf;

fn examples_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Examples")
}

/// Every test here is driven by real vendor preset files, which are other
/// people's factory presets and so are deliberately not committed. A clone
/// without them — CI included — cannot run these tests, so skip loudly
/// instead of failing. They are the real safety net and must be run locally,
/// where the fixtures exist, before trusting a change.
macro_rules! require_examples {
    () => {
        if !examples_root().is_dir() {
            eprintln!("SKIPPED: no Examples/ fixtures in this checkout (see .gitignore)");
            return;
        }
    };
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../Examples/Neural DSP")
        .join(name)
}

fn template_text() -> String {
    fs::read_to_string(example("Archetype John Mayer X VST3 by Scott.RPL")).unwrap()
}

#[test]
fn parses_valuetree_metadata() {
    require_examples!();
    let data = fs::read(example("Golden Gate.xml")).unwrap();
    let vt = valuetree::parse(&data).unwrap();
    assert_eq!(vt.name, "mayer");
    assert_eq!(vt.prop("name").unwrap().as_text(), "Golden Gate");
    let tags = vt.child("tags").expect("tags node");
    assert_eq!(tags.children.len(), 7);
    let app = vt.child("appModel").expect("appModel node");
    assert_eq!(app.prop("selectedAmp").unwrap().as_text(), "1");
}

#[test]
fn rpl_writer_matches_reaper_formatting_exactly() {
    require_examples!();
    let text = template_text();
    let lib = rpl::parse(&text).unwrap();
    assert_eq!(lib.presets.len(), 3);
    let rebuilt = rpl::write(&lib.header, &lib.presets);
    assert_eq!(rebuilt, text, "re-serializing REAPER's own RPL must be lossless");
}

#[test]
fn golden_gate_round_trips_byte_identical() {
    require_examples!();
    let template = load_template(&template_text()).unwrap();
    let ground_truth = rpl::parse(&template_text()).unwrap();
    let data = fs::read(example("Golden Gate.xml")).unwrap();
    let converted = convert_preset(&template, &data, "fallback unused").unwrap();
    assert_eq!(converted.name, "Golden Gate");
    assert!(converted.parameters_applied > 100, "expected a full parameter set");
    assert_eq!(
        converted.data, ground_truth.presets[0].data,
        "converting the template's own source preset must reproduce REAPER's blob byte-for-byte"
    );
}

#[test]
fn gravity_presets_match_reaper_saved_state() {
    require_examples!();
    let template = load_template(&template_text()).unwrap();
    let ground_truth = rpl::parse(&template_text()).unwrap();
    for (file, index) in [("Gravity Clean.xml", 1), ("Gravity Lead.xml", 2)] {
        let data = fs::read(example(file)).unwrap();
        let converted = convert_preset(&template, &data, file).unwrap();
        let expected = &ground_truth.presets[index];
        assert_eq!(converted.name, expected.name, "{file}: preset name");
        assert_eq!(
            normalize(&extract_xml(&converted.data)),
            normalize(&extract_xml(&expected.data)),
            "{file}: parameter state must match what REAPER captured"
        );
    }
}

#[test]
fn built_rpl_parses_back_with_all_presets() {
    require_examples!();
    let template = load_template(&template_text()).unwrap();
    let converted: Vec<_> = ["Golden Gate.xml", "Gravity Clean.xml", "Gravity Lead.xml"]
        .iter()
        .map(|f| convert_preset(&template, &fs::read(example(f)).unwrap(), f).unwrap())
        .collect();
    let out = build_rpl(&template, &converted);
    let parsed = rpl::parse(&out).unwrap();
    assert_eq!(parsed.header, template.library_header);
    let names: Vec<_> = parsed.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Golden Gate", "Gravity Clean", "Gravity Lead"]);
    assert_eq!(parsed.presets[0].data, converted[0].data);
}

#[test]
fn batch_sorts_case_insensitively_and_marks_folder_starts() {
    require_examples!();
    // Build a scratch layout mixing top-level files, a subfolder, and a
    // nested subfolder:
    //   batch/Ambient/dark Cave.xml    (Gravity Lead)
    //   batch/Ambient/Spacey/deep.xml  (Golden Gate)
    //   batch/apple.xml                (Golden Gate)
    //   batch/Zebra.xml                (Gravity Clean)
    // Case-insensitive order is Ambient, apple, Zebra; a case-sensitive sort
    // would wrongly put Zebra before apple.
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("batch");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("Ambient/Spacey")).unwrap();
    fs::copy(example("Gravity Lead.xml"), root.join("Ambient/dark Cave.xml")).unwrap();
    fs::copy(example("Golden Gate.xml"), root.join("Ambient/Spacey/deep.xml")).unwrap();
    fs::copy(example("Golden Gate.xml"), root.join("apple.xml")).unwrap();
    fs::copy(example("Gravity Clean.xml"), root.join("Zebra.xml")).unwrap();

    let files = rplmaker_core::files::collect_preset_files(&[root]).unwrap();
    let subfolders: Vec<_> = files.iter().map(|f| f.subfolder.clone()).collect();
    assert_eq!(
        subfolders,
        [
            Some(vec!["Ambient".to_string()]),
            Some(vec!["Ambient".to_string(), "Spacey".to_string()]),
            None,
            None
        ]
    );

    let template = load_template(&template_text()).unwrap();
    let flat = convert_files(&template, &files, FolderNaming::Flat);
    assert!(flat.skipped.is_empty());
    let flat_names: Vec<_> = flat.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        flat_names,
        ["Gravity Lead", "Golden Gate", "Golden Gate", "Gravity Clean"]
    );

    let deepest = convert_files(&template, &files, FolderNaming::Deepest);
    let deepest_names: Vec<_> = deepest.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        deepest_names,
        [
            "Ambient folder: Gravity Lead",
            "Spacey folder: Golden Gate",
            "Golden Gate",
            "Gravity Clean"
        ],
        "deepest naming uses only the innermost folder name"
    );

    let full = convert_files(&template, &files, FolderNaming::FullPath);
    let full_names: Vec<_> = full.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        full_names,
        [
            "Ambient folder: Gravity Lead",
            "Ambient, Spacey folder: Golden Gate",
            "Golden Gate",
            "Gravity Clean"
        ],
        "full-path naming spells out nested folders"
    );

    // The marker name must also be embedded inside the blob footer, not
    // just in the RPL entry, so the output stays usable as a template.
    let mut embedded = b"Ambient folder: Gravity Lead".to_vec();
    embedded.push(0);
    assert!(
        find(&deepest.presets[0].data, &embedded).is_some(),
        "blob footer must carry the marker name"
    );
    let out = build_rpl(&template, &deepest.presets);
    let reloaded = load_template(&out)
        .expect("a marker-converted RPL must itself load as a template");
    assert_eq!(reloaded.library_header, template.library_header);
}

#[test]
fn template_loads_even_when_entry_name_mismatches_footer() {
    require_examples!();
    // Simulates RPLs produced before the marker-naming fix, where the RPL
    // entry name and the blob's embedded name diverged.
    let mut lib = rpl::parse(&template_text()).unwrap();
    lib.presets[0].name = "Totally Renamed".to_string();
    let rewritten = rpl::write(&lib.header, &lib.presets);
    load_template(&rewritten)
        .expect("structural footer-name fallback should handle renamed entries");
}

#[test]
fn valuetree_writer_round_trips_vendor_files() {
    require_examples!();
    for file in ["Golden Gate.xml", "Gravity Clean.xml", "Gravity Lead.xml"] {
        let data = fs::read(example(file)).unwrap();
        let tree = valuetree::parse(&data).unwrap();
        assert_eq!(
            valuetree::write(&tree),
            data,
            "{file}: parse + write must reproduce the file byte-for-byte"
        );
    }
}

#[test]
fn gojira_valuetree_state_loads_as_template() {
    require_examples!();
    let text = fs::read_to_string(example("Gojira.RPL")).unwrap();
    let template = load_template(&text).expect("ValueTree-state template must load");
    assert_eq!(
        template.library_header,
        "\"VST3: Archetype Gojira (Neural DSP)\""
    );
}

#[test]
fn gojira_self_conversion_reproduces_state() {
    require_examples!();
    // No vendor preset file for Gojira is available, so feed the template's
    // own state tree back in as a synthetic binary preset file: every value
    // then overlays onto itself and the merged state must come out
    // byte-identical.
    let text = fs::read_to_string(example("Gojira.RPL")).unwrap();
    let lib = rpl::parse(&text).unwrap();
    let blob = rplmaker_core::blob::Blob::parse(&lib.presets[0].data, &lib.presets[0].name).unwrap();
    let template = load_template(&text).unwrap();

    let converted = convert_preset(&template, &blob.state, "fallback unused").unwrap();
    // The state tree carries presetNameProp "Default", which becomes the name.
    assert_eq!(converted.name, "Default");
    assert!(converted.parameters_applied > 200, "root props + 120 PARAM nodes");
    assert_eq!(
        converted.data,
        blob.rebuild(&blob.state, "Default"),
        "self-conversion must reproduce the plugin state byte-for-byte"
    );
}

#[test]
fn xml_text_preset_files_are_accepted() {
    require_examples!();
    // The Mayer template's state is XML text; feeding that text back as a
    // vendor preset file exercises the XML reader end to end. Its values
    // are Golden Gate's, so the result must equal the Golden Gate blob.
    let template = load_template(&template_text()).unwrap();
    let lib = rpl::parse(&template_text()).unwrap();
    let blob = rplmaker_core::blob::Blob::parse(&lib.presets[0].data, &lib.presets[0].name).unwrap();

    let converted = convert_preset(&template, &blob.state, "Golden Gate").unwrap();
    assert_eq!(converted.name, "Golden Gate", "appModel has no name; fallback applies");
    assert_eq!(
        converted.data, lib.presets[0].data,
        "XML-text preset input must reproduce REAPER's blob byte-for-byte"
    );
}

#[test]
fn library_edits_reorder_rename_and_duplicate() {
    require_examples!();
    use rplmaker_core::edit::{apply_edits, PresetEdit};
    let text = template_text();
    let edit = |original_index: usize, name: &str| PresetEdit {
        original_index,
        name: name.to_string(),
    };

    // Reverse order, rename one, duplicate another, drop the third.
    let edited = apply_edits(
        &text,
        &[
            edit(2, "Gravity Lead"),
            edit(0, "Golden Gate (Mayer)"),
            edit(0, "Golden Gate copy"),
        ],
    )
    .unwrap();
    let lib = rpl::parse(&edited).unwrap();
    let names: Vec<_> = lib.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Gravity Lead", "Golden Gate (Mayer)", "Golden Gate copy"]);

    // Untouched presets are copied verbatim; renamed ones carry the new
    // name inside the blob footer as well.
    let original = rpl::parse(&text).unwrap();
    assert_eq!(lib.presets[0].data, original.presets[2].data);
    let mut embedded = b"Golden Gate (Mayer)".to_vec();
    embedded.push(0);
    assert!(
        find(&lib.presets[1].data, &embedded).is_some(),
        "rename must reach the blob footer"
    );

    // The edited library still works as a conversion template.
    load_template(&edited).expect("edited library must remain a valid template");

    // A no-op edit list reproduces the file byte-for-byte.
    let noop = apply_edits(
        &text,
        &[
            edit(0, "Golden Gate"),
            edit(1, "Gravity Clean"),
            edit(2, "Gravity Lead"),
        ],
    )
    .unwrap();
    assert_eq!(noop, text);

    // Guard rails: empty library and empty names are refused.
    assert!(apply_edits(&text, &[]).is_err());
    assert!(apply_edits(&text, &[edit(0, "  ")]).is_err());
    assert!(apply_edits(&text, &[edit(9, "x")]).is_err());
}

#[test]
fn ampbox_template_self_conversion_is_byte_identical() {
    require_examples!();
    // Ampbox stores its XML state APVTS-style: every parameter is a PARAM
    // element distinguished only by its id attribute. This test guards the
    // id-aware child matching in xmlmerge; name-only matching would map
    // every PARAM onto the first one and corrupt the values.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Examples/AmpBox.RPL");
    let text = fs::read_to_string(path).unwrap();
    let template = load_template(&text).expect("Ampbox template must load");
    assert_eq!(template.library_header, "\"VST3: Ampbox (Mercuriall)\"");

    let lib = rpl::parse(&text).unwrap();
    let blob = rplmaker_core::blob::Blob::parse(&lib.presets[0].data, &lib.presets[0].name).unwrap();
    let converted = convert_preset(&template, &blob.state, "Default settings").unwrap();
    assert!(converted.parameters_applied > 100);
    assert_eq!(
        converted.data, lib.presets[0].data,
        "feeding the template's own Euphoria XML back must reproduce the blob byte-for-byte"
    );
}

#[test]
fn embedded_scan_finds_xml_states_and_skips_truncated_ones() {
    require_examples!();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Examples/AmpBox.RPL");
    let text = fs::read_to_string(path).unwrap();
    let template = load_template(&text).unwrap();
    assert_eq!(template.state_root_name(), "Euphoria");

    let lib = rpl::parse(&text).unwrap();
    let blob = rplmaker_core::blob::Blob::parse(&lib.presets[0].data, &lib.presets[0].name).unwrap();
    let state = String::from_utf8(blob.state.clone()).unwrap();
    let mutated = state.replace("page_id=\"1\"", "page_id=\"2\"");
    assert_ne!(state, mutated, "mutation must change the document");

    // Synthetic binary: noise, one state, padding, a mutated copy, an exact
    // duplicate, then a truncated document that runs into another copy.
    let mut binary = Vec::new();
    binary.extend_from_slice(&[0u8; 64]);
    binary.extend_from_slice(b"noise");
    binary.extend_from_slice(state.as_bytes());
    binary.extend_from_slice(&[0u8; 16]);
    binary.extend_from_slice(mutated.as_bytes());
    binary.extend_from_slice(state.as_bytes()); // duplicate
    binary.extend_from_slice(b"<Euphoria truncated=\"1\" ");
    binary.extend_from_slice(state.as_bytes()); // truncation detector target

    let docs = rplmaker_core::extract::extract_embedded_presets(&template, &binary);
    assert_eq!(docs.len(), 2, "one original, one mutated; duplicate and truncated skipped");
    for doc in &docs {
        convert_preset(&template, doc, "scanned").expect("scanned docs must convert");
    }
}

#[test]
fn embedded_scan_finds_valuetree_states() {
    require_examples!();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../Examples/Neural DSP/Gojira.RPL");
    let text = fs::read_to_string(path).unwrap();
    let template = load_template(&text).unwrap();
    assert_eq!(template.state_root_name(), "neural_dsp_gojira");

    let lib = rpl::parse(&text).unwrap();
    let blob = rplmaker_core::blob::Blob::parse(&lib.presets[0].data, &lib.presets[0].name).unwrap();
    let (mut node, _) = valuetree::parse_with_len(&blob.state).unwrap();
    node.props[0].1 = valuetree::Value::Int(3); // editorSize: make a distinct doc
    let mutated = valuetree::write(&node);

    let mut binary = Vec::new();
    binary.extend_from_slice(b"junk");
    binary.extend_from_slice(&blob.state);
    binary.extend_from_slice(&[0u8; 32]);
    binary.extend_from_slice(&mutated);
    binary.extend_from_slice(&blob.state); // duplicate

    let docs = rplmaker_core::extract::extract_embedded_presets(&template, &binary);
    assert_eq!(docs.len(), 2);
    for doc in &docs {
        convert_preset(&template, doc, "scanned").expect("scanned trees must convert");
    }
}

#[test]
fn rejects_preset_from_a_different_plugin() {
    require_examples!();
    let template = load_template(&template_text()).unwrap();
    // A structurally valid ValueTree whose node/property names match nothing:
    // node "zzz" with one string property, zero children.
    let alien = b"zzz\0\x01\x01nope\0\x01\x03\x05ok\0\0\0".to_vec();
    let result = convert_preset(&template, &alien, "alien");
    assert!(result.is_err(), "presets for other plugins must be rejected");
}

// --- Universal Audio (UADx): UBJSON state, JSON preset files ---------------

fn ua_template_text() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Examples/Universal Audio.RPL");
    fs::read_to_string(path).unwrap()
}

fn ua_example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../Examples/Universal Audio")
        .join(name)
}

#[test]
fn ubjson_round_trips_template_state_byte_identical() {
    require_examples!();
    use rplmaker_core::{blob::Blob, ubjson};
    let lib = rpl::parse(&ua_template_text()).unwrap();
    let blob = Blob::parse(&lib.presets[0].data, &lib.presets[0].name).unwrap();
    let (value, len) = ubjson::parse(&blob.state).unwrap();
    assert_eq!(len, blob.state.len(), "the UBJSON must consume the whole state");
    assert_eq!(
        ubjson::write(&value),
        blob.state,
        "re-writing a parsed UA state must reproduce it byte-for-byte"
    );
}

#[test]
fn ua_template_loads_as_ubjson() {
    require_examples!();
    // A UA template must load without being mistaken for XML or a ValueTree.
    let template = load_template(&ua_template_text()).unwrap();
    assert!(
        template.library_header.contains("Universal Audio"),
        "header: {}",
        template.library_header
    );
}

#[test]
fn converts_ua_factory_preset() {
    require_examples!();
    use rplmaker_core::{blob::Blob, json, ubjson};
    let template = load_template(&ua_template_text()).unwrap();
    let data = fs::read(ua_example("Vocal_Butter.json")).unwrap();
    let converted = convert_preset(&template, &data, "fallback unused").unwrap();
    assert_eq!(converted.name, "Vocal Butter");
    assert!(converted.parameters_applied >= 3, "expected the control set");

    // The output must itself be a valid REAPER blob (usable as a template).
    let reblob = Blob::parse(&converted.data, &converted.name).unwrap();
    let (state, _) = ubjson::parse(&reblob.state).unwrap();
    assert_eq!(state.get("preset_name").and_then(ubjson::Value::as_str), Some("Vocal Butter"));

    // The embedded payload JSON must equal the file's chunk, and the same
    // preset must be mirrored into the alternate slot.
    let payload = state.get("plugin_state_payload").and_then(ubjson::Value::as_bytes).unwrap();
    assert_eq!(payload.last(), Some(&0), "payload must stay null-terminated like the plugin's own");
    let payload_text = std::str::from_utf8(payload).unwrap().trim_end_matches('\0');
    let payload_json = json::parse(payload_text).unwrap();
    let file_members = json::object_members_raw(std::str::from_utf8(&data).unwrap()).unwrap();
    let chunk_raw = &file_members.iter().find(|(k, _)| k == "chunk").unwrap().1;
    assert_eq!(payload_json, json::parse(chunk_raw).unwrap(), "payload must match the file's chunk");

    let alt = state.get("alternate_state").unwrap();
    assert_eq!(alt.get("preset_name").and_then(ubjson::Value::as_str), Some("Vocal Butter"));
    assert_eq!(
        alt.get("plugin_state_payload").and_then(ubjson::Value::as_bytes),
        Some(payload),
        "alternate slot must carry the same payload"
    );
}

#[test]
fn ua_batch_builds_a_loadable_library() {
    require_examples!();
    use rplmaker_core::blob::Blob;
    let template = load_template(&ua_template_text()).unwrap();
    let files = [
        "Acoustic_Shimmer.json",
        "Default.json",
        "Drum_Bus_Master_Tape.json",
        "Vocal_Butter.json",
    ];
    let converted: Vec<_> = files
        .iter()
        .map(|f| convert_preset(&template, &fs::read(ua_example(f)).unwrap(), f).unwrap())
        .collect();
    let out = build_rpl(&template, &converted);
    let parsed = rpl::parse(&out).unwrap();
    assert_eq!(parsed.header, template.library_header);
    let names: Vec<_> = parsed.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Acoustic Shimmer", "Default", "Drum Bus Master Tape", "Vocal Butter"]);
    for p in &parsed.presets {
        Blob::parse(&p.data, &p.name).expect("each converted UA preset must be a valid blob");
    }
}

#[test]
fn rejects_ua_preset_with_foreign_controls() {
    require_examples!();
    let template = load_template(&ua_template_text()).unwrap();
    // Valid UA-shaped JSON, but its controls belong to no UADx plugin here.
    let alien = br#"{"chunk":{"controls":{"nonsense_knob":{"real_value":1}},"ua_chunk_version":1},"name":"Alien","plugin_id":"someone_else","uid":"x","version":1}"#;
    let result = convert_preset(&template, alien, "alien");
    assert!(result.is_err(), "a UA preset with unknown controls must be rejected");
}

fn extract_xml(blob: &[u8]) -> String {
    let start = find(blob, b"<?xml").expect("xml start");
    let end = find(blob, b"</appModel>").expect("xml end") + "</appModel>".len();
    String::from_utf8(blob[start..end].to_vec()).unwrap()
}

/// presetUid is a random id minted when a preset is captured; it does not
/// exist in the vendor preset file, so the converter carries the template's
/// value. Blank it on both sides before comparing.
fn normalize(xml: &str) -> String {
    match xml.find("presetUid=\"") {
        Some(p) => {
            let value_start = p + "presetUid=\"".len();
            let value_end = value_start + xml[value_start..].find('"').unwrap();
            format!("{}{}", &xml[..value_start], &xml[value_end..])
        }
        None => xml.to_string(),
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
