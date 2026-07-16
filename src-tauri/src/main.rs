#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize)]
struct Summary {
    library: String,
    converted: Vec<ConvertedInfo>,
    skipped: Vec<String>,
    output: String,
}

#[derive(Serialize)]
struct ConvertedInfo {
    name: String,
    parameters: usize,
}

#[derive(Serialize)]
struct ScanInfo {
    preset_files: usize,
    has_subfolders: bool,
}

/// Report how many preset files the chosen inputs contain and whether any
/// sit in subfolders, so the UI can reveal the folder-handling choice only
/// when it matters.
#[tauri::command]
fn scan_inputs(inputs: Vec<String>) -> Result<ScanInfo, String> {
    let paths: Vec<PathBuf> = inputs.into_iter().map(PathBuf::from).collect();
    let files = rplmaker_core::files::collect_preset_files(&paths).map_err(|e| e.to_string())?;
    Ok(ScanInfo {
        preset_files: files.len(),
        has_subfolders: files.iter().any(|f| f.subfolder.is_some()),
    })
}

/// Validate a template RPL and report which plugin library it targets, so
/// the UI can confirm the choice immediately after the file picker closes.
#[tauri::command]
fn inspect_template(template: String) -> Result<String, String> {
    let text = std::fs::read_to_string(&template)
        .map_err(|e| format!("cannot read the template file: {e}"))?;
    let t = rplmaker_core::load_template(&text).map_err(|e| e.to_string())?;
    Ok(t.library_header)
}

#[tauri::command]
fn convert(
    template: String,
    inputs: Vec<String>,
    output: String,
    folder_naming: String,
) -> Result<Summary, String> {
    let naming = match folder_naming.as_str() {
        "deepest" => rplmaker_core::FolderNaming::Deepest,
        "full" => rplmaker_core::FolderNaming::FullPath,
        _ => rplmaker_core::FolderNaming::Flat,
    };
    let text = std::fs::read_to_string(&template)
        .map_err(|e| format!("cannot read the template file: {e}"))?;
    let t = rplmaker_core::load_template(&text).map_err(|e| e.to_string())?;

    let paths: Vec<PathBuf> = inputs.into_iter().map(PathBuf::from).collect();
    let files = rplmaker_core::files::collect_preset_files(&paths).map_err(|e| e.to_string())?;
    if files.is_empty() {
        return Err("No preset files were found in the chosen files or folders.".into());
    }

    let outcome = rplmaker_core::convert_files(&t, &files, naming);
    let skipped: Vec<String> = outcome
        .skipped
        .iter()
        .map(|(path, e)| format!("{}: {e}", path.display()))
        .collect();
    if outcome.presets.is_empty() {
        return Err(format!(
            "No presets could be converted. First problem: {}",
            skipped.first().cloned().unwrap_or_default()
        ));
    }

    let out_text = rplmaker_core::build_rpl(&t, &outcome.presets);
    std::fs::write(&output, out_text).map_err(|e| format!("cannot write the output file: {e}"))?;

    Ok(Summary {
        library: t.library_header,
        converted: outcome
            .presets
            .iter()
            .map(|p| ConvertedInfo { name: p.name.clone(), parameters: p.parameters_applied })
            .collect(),
        skipped,
        output,
    })
}

#[derive(Serialize)]
struct PluginScanInfo {
    folder: String,
    found: usize,
}

/// Scan a plugin binary for embedded factory presets and stage them as
/// preset files in a temp folder the normal conversion flow can consume.
#[tauri::command]
fn scan_plugin(template: String, plugin: String) -> Result<PluginScanInfo, String> {
    let text = std::fs::read_to_string(&template)
        .map_err(|e| format!("cannot read the template file: {e}"))?;
    let t = rplmaker_core::load_template(&text).map_err(|e| e.to_string())?;
    let binary =
        std::fs::read(&plugin).map_err(|e| format!("cannot read the plug-in file: {e}"))?;
    let docs = rplmaker_core::extract::extract_embedded_presets(&t, &binary);
    if docs.is_empty() {
        return Err(format!(
            "no embedded presets found; the scan looked for '{}' state documents, and this \
             plug-in may compress its resources or keep presets elsewhere",
            t.state_root_name()
        ));
    }
    let stem = std::path::Path::new(&plugin)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Plug-in".to_string());
    let dir = std::env::temp_dir().join(format!("rplmaker-extracted-{stem}"));
    rplmaker_core::extract::write_extracted(&dir, &stem, &docs).map_err(|e| e.to_string())?;
    Ok(PluginScanInfo {
        folder: dir.to_string_lossy().into_owned(),
        found: docs.len(),
    })
}

#[derive(Serialize)]
struct LibraryInfo {
    header: String,
    names: Vec<String>,
}

/// List an existing RPL's presets for the editor.
#[tauri::command]
fn load_library(path: String) -> Result<LibraryInfo, String> {
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read the library file: {e}"))?;
    let info = rplmaker_core::edit::read_library(&text).map_err(|e| e.to_string())?;
    Ok(LibraryInfo { header: info.header, names: info.names })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresetEditArg {
    original_index: usize,
    name: String,
}

/// Apply the editor's reorder/rename list and write the library back to
/// disk (via a temp file, so a failed write can't corrupt the original).
#[tauri::command]
fn save_library(path: String, edits: Vec<PresetEditArg>) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read the library file: {e}"))?;
    let edits: Vec<rplmaker_core::edit::PresetEdit> = edits
        .into_iter()
        .map(|e| rplmaker_core::edit::PresetEdit { original_index: e.original_index, name: e.name })
        .collect();
    let new_text = rplmaker_core::edit::apply_edits(&text, &edits).map_err(|e| e.to_string())?;
    rplmaker_core::files::write_atomically(std::path::Path::new(&path), &new_text)
        .map_err(|e| format!("cannot write the library file: {e}"))?;
    Ok(edits.len())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // WebView2 does not reliably take keyboard focus when the window
            // opens, which leaves screen readers with nothing to land on.
            // Focus the window explicitly; the page then moves focus to its
            // heading (see ui/main.js).
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
            Ok(())
        })
        // When the window opens in the background (e.g. launched via
        // `cargo run` from a terminal that keeps the foreground), WebView2
        // does not take keyboard focus on later activation either. Re-assert
        // it every time the window becomes focused; if the webview already
        // has focus this is a no-op.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(true) = event {
                let _ = window.set_focus();
            }
        })
        .invoke_handler(tauri::generate_handler![
            scan_inputs,
            scan_plugin,
            inspect_template,
            convert,
            load_library,
            save_library
        ])
        .run(tauri::generate_context!())
        .expect("failed to start rplMaker");
}
