//! Editing existing RPL libraries: renaming, reordering, removing, or
//! duplicating presets without touching their parameter state.

use crate::blob::Blob;
use crate::rpl::{self, Preset};
use crate::{err, Error, Result};

pub struct LibraryInfo {
    pub header: String,
    pub names: Vec<String>,
}

pub fn read_library(text: &str) -> Result<LibraryInfo> {
    let lib = rpl::parse(text)?;
    Ok(LibraryInfo {
        header: lib.header,
        names: lib.presets.iter().map(|p| p.name.clone()).collect(),
    })
}

/// One entry of the edited library, pointing back at the preset it came
/// from. Listing an index twice duplicates that preset; omitting an index
/// drops it.
pub struct PresetEdit {
    pub original_index: usize,
    pub name: String,
}

/// Apply reorder/rename edits and return the new RPL text. Renames are
/// applied inside the preset blob as well as on the RPL entry: the two must
/// stay in sync for the file to keep working as a conversion template.
pub fn apply_edits(text: &str, edits: &[PresetEdit]) -> Result<String> {
    let lib = rpl::parse(text)?;
    if edits.is_empty() {
        return err("the edited library would contain no presets");
    }
    let mut presets = Vec::with_capacity(edits.len());
    for edit in edits {
        let source = lib.presets.get(edit.original_index).ok_or_else(|| {
            Error(format!("preset index {} is out of range", edit.original_index))
        })?;
        let name = edit.name.trim();
        if name.is_empty() {
            return err(format!("preset '{}' would get an empty name", source.name));
        }
        if name == source.name {
            presets.push(Preset {
                name: source.name.clone(),
                data: source.data.clone(),
            });
        } else {
            let blob = Blob::parse(&source.data, &source.name)?;
            presets.push(Preset {
                name: name.to_string(),
                data: blob.rebuild(&blob.state, name),
            });
        }
    }
    Ok(rpl::write(&lib.header, &presets))
}
