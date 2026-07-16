//! Filesystem helpers shared by the CLI and the GUI.

use std::io;
use std::path::{Path, PathBuf};

pub struct CollectedFile {
    pub path: PathBuf,
    /// Folder components of the subfolder the file sits in, relative to the
    /// top-level input it came from (["Ambient"], or ["Ambient", "Spacey"]
    /// when nested). None for files passed directly or sitting at the top
    /// of an input folder.
    pub subfolder: Option<Vec<String>>,
}

/// Expand a mix of files and directories into a flat list of candidate
/// preset files, sorted case-insensitively by path so presets stay grouped
/// by subfolder in a predictable order. RPL files are skipped so pointing at
/// a folder that also holds the template or previous output does no harm.
pub fn collect_preset_files(inputs: &[PathBuf]) -> io::Result<Vec<CollectedFile>> {
    let mut out = Vec::new();
    for input in inputs {
        if input.is_dir() {
            visit_dir(input, input, &mut out)?;
        } else if is_candidate(input) {
            out.push(CollectedFile { path: input.clone(), subfolder: None });
        }
    }
    out.sort_by(|a, b| sort_key(&a.path).cmp(&sort_key(&b.path)));
    Ok(out)
}

fn visit_dir(root: &Path, dir: &Path, out: &mut Vec<CollectedFile>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            visit_dir(root, &path, out)?;
        } else if is_candidate(&path) {
            let subfolder = path
                .parent()
                .filter(|parent| *parent != root)
                .and_then(|parent| parent.strip_prefix(root).ok())
                .map(|rel| {
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                });
            out.push(CollectedFile { path, subfolder });
        }
    }
    Ok(())
}

/// Write via a sibling temp file and rename, so a crash or full disk
/// mid-write can't destroy an existing file being edited in place.
pub fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let tmp = path.with_file_name(format!("{file_name}.tmp"));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

fn is_candidate(path: &Path) -> bool {
    path.extension()
        .map_or(true, |e| !e.eq_ignore_ascii_case("rpl"))
}

/// Compare path components lowercased, so ordering ignores case and never
/// depends on how the path separator compares against letters.
fn sort_key(path: &Path) -> Vec<String> {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}
