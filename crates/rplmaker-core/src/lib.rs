//! rplmaker-core: converts plugin preset files into REAPER RPL preset
//! libraries, so screen reader users can browse presets through REAPER's
//! accessible native preset combobox instead of an inaccessible plugin GUI.
//!
//! The approach is template-based: the user saves one preset for the target
//! plugin through REAPER itself and exports it as an RPL. That template
//! teaches the converter every plugin-specific wrapper byte; conversion then
//! swaps the plugin's parameter state (XML) inside the wrapper and adjusts
//! the length fields.

pub mod blob;
pub mod convert;
pub mod edit;
pub mod extract;
pub mod files;
pub mod json;
pub mod rpl;
pub mod treemerge;
pub mod uapreset;
pub mod ubjson;
pub mod valuetree;
pub mod xmlmerge;
pub mod xmlread;

pub use convert::{
    build_rpl, convert_files, convert_preset, load_template, BatchOutcome, ConvertedPreset,
    FolderNaming, Template,
};

use std::fmt;

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}
