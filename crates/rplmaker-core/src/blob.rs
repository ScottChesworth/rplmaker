//! REAPER's binary wrapper around a VST3 plugin state, as found inside RPL
//! preset blobs and RPP project files.
//!
//! Layout (little-endian throughout):
//!   4  first bytes of the plugin id
//!   4  magic 0xFEED5EEE (VST3)
//!   4  input channel count, then 8 bytes of routing mask per input
//!   4  output channel count, then 8 bytes of routing mask per output
//!   4  payload length
//!   4  constant 1
//!   payload:
//!     4  program field (0x0000FFFF)
//!     4  component state length
//!     4  constant 1
//!     component state: for JUCE plugins "VC2!" + u32 XML length + XML text
//!                      + a constant trailer (JUCEPrivateData block)
//!     4  controller state length (0 in every observed preset) + data
//!   footer: padding, preset name, null, padding
//!
//! Only the three length fields and the XML/name vary between presets of the
//! same plugin, so everything else is carried over verbatim from a template
//! preset that the user saved through REAPER once.

use crate::{err, Error, Result};

const REAPER_VST3_MAGIC: u32 = 0xFEED_5EEE;
const JUCE_XML_MAGIC: &[u8; 4] = b"VC2!";
const MAX_CHANNELS: usize = 256;

/// How the plugin serializes its state inside the component chunk.
pub enum StateKind {
    /// XML text (JUCE's "VC2!" form, possibly under further wrappers).
    Xml,
    /// A raw JUCE binary ValueTree (older JUCE plugins).
    Tree,
    /// A UBJSON object (Universal Audio's UADx plugins).
    Ubjson,
}

pub struct Blob {
    head: Vec<u8>,
    post_len: [u8; 4],
    prog: [u8; 4],
    mid: [u8; 4],
    /// Wrapper bytes between the component start and the state: empty for
    /// bare ValueTree states, "VC2!"+length for plain JUCE XML, and up to
    /// VstW + CcnK/FBCh fxBank headers for VST2-compatibility states.
    comp_head: Vec<u8>,
    /// Offsets within comp_head of u32 length fields that must shift when
    /// the state size changes, in each byte order.
    be_len_fields: Vec<usize>,
    le_len_fields: Vec<usize>,
    /// The plugin state: XML text or binary ValueTree bytes, per `kind`.
    pub state: Vec<u8>,
    pub kind: StateKind,
    comp_trailer: Vec<u8>,
    payload_tail: Vec<u8>,
    footer_prefix: Vec<u8>,
    footer_suffix: Vec<u8>,
    comp_len_field: u32,
    payload_len_field: u32,
}

impl Blob {
    /// Dissect a template preset blob. `preset_name` is the name the preset
    /// was saved under, used to locate the name inside the footer.
    pub fn parse(data: &[u8], preset_name: &str) -> Result<Blob> {
        if read_u32(data, 4)? != REAPER_VST3_MAGIC {
            return err(
                "the template preset is not in REAPER's VST3 format; \
                 save the template from the VST3 version of the plugin",
            );
        }
        let num_in = read_u32(data, 8)? as usize;
        if num_in > MAX_CHANNELS {
            return err("implausible input channel count in template preset");
        }
        let mut off = 12 + num_in * 8;
        let num_out = read_u32(data, off)? as usize;
        if num_out > MAX_CHANNELS {
            return err("implausible output channel count in template preset");
        }
        off += 4 + num_out * 8;

        let payload_len_field = read_u32(data, off)?;
        let head = data[..off].to_vec();
        let post_len = slice4(data, off + 4)?;
        let payload_start = off + 8;
        let payload = data
            .get(payload_start..payload_start + payload_len_field as usize)
            .ok_or_else(|| Error("template preset payload is truncated".into()))?;
        if payload.len() < 16 {
            return err("template preset payload is too short");
        }

        let prog = slice4(payload, 0)?;
        let comp_len_field = read_u32(payload, 4)?;
        let mid = slice4(payload, 8)?;
        let component = payload
            .get(12..12 + comp_len_field as usize)
            .ok_or_else(|| Error("template component state is truncated".into()))?;
        let payload_tail = payload[12 + comp_len_field as usize..].to_vec();

        // Peel wrappers off the component until the actual state is
        // reached, recording every length field that covers the state.
        let mut p = 0usize;
        let mut be_len_fields = Vec::new();
        let mut le_len_fields = Vec::new();

        // Steinberg VST2-compatibility header: 'VstW' + BE size + BE
        // version + BE bypass; none of the fields depend on state size.
        if component.get(..4) == Some(b"VstW".as_slice()) {
            p = 16;
            // Then usually a VST2 fxBank/fxProgram chunk structure.
            if component.get(p..p + 4) == Some(b"CcnK".as_slice()) {
                be_len_fields.push(p + 4); // structure size after this field
                let fx_magic = component
                    .get(p + 8..p + 12)
                    .ok_or_else(|| Error("truncated fx chunk in template state".into()))?;
                // Fixed-size headers per format, ending with a BE chunkSize
                // just before the wrapped data. FBCh: version, fxID,
                // fxVersion, numPrograms, future[128]. FPCh: version, fxID,
                // fxVersion, numParams, prgName[28].
                let header_len = match fx_magic {
                    b"FBCh" => 16 + 128,
                    b"FPCh" => 16 + 28,
                    _ => return err("unrecognized fx chunk format in template state"),
                };
                let size_at = p + 12 + header_len;
                be_len_fields.push(size_at);
                p = size_at + 4;
            }
        }

        let inner = component
            .get(p..)
            .ok_or_else(|| Error("template component state is truncated".into()))?;
        let (kind, state_start, state_len) = if inner.get(..4) == Some(JUCE_XML_MAGIC.as_slice())
        {
            let xml_len = read_u32(inner, 4)? as usize;
            le_len_fields.push(p + 4);
            if inner.len() < 8 + xml_len || !inner[8..].starts_with(b"<") {
                return err("template plugin state does not look like XML");
            }
            (StateKind::Xml, p + 8, xml_len)
        } else if inner.first() == Some(&b'{') {
            // Universal Audio's UADx plugins store a UBJSON object here.
            match crate::ubjson::parse(inner) {
                Ok((_, len)) if len >= 8 => (StateKind::Ubjson, p, len),
                _ => {
                    return err(
                        "the plugin's state opens like UBJSON but could not be parsed; \
                         this plugin needs its own converter support",
                    )
                }
            }
        } else {
            match crate::valuetree::parse_with_len(inner) {
                // Guard against garbage that happens to scan as a tiny
                // empty tree, which would silently match nothing later.
                Ok((node, len)) if len >= 32 && (!node.props.is_empty() || !node.children.is_empty()) => {
                    (StateKind::Tree, p, len)
                }
                _ => {
                    return err(
                        "the plugin's state format is not recognized (neither JUCE XML \
                         nor a JUCE ValueTree); this plugin needs its own converter support",
                    )
                }
            }
        };
        let comp_head = component[..state_start].to_vec();
        let state = component[state_start..state_start + state_len].to_vec();
        let comp_trailer = component[state_start + state_len..].to_vec();

        let footer = &data[payload_start + payload_len_field as usize..];
        let mut name_pattern = preset_name.as_bytes().to_vec();
        name_pattern.push(0);
        // Prefer an exact match on the RPL entry name; fall back to locating
        // the name slot structurally, which copes with files whose entry
        // name and embedded name diverge (e.g. output of rplmaker versions
        // that applied folder markers after building the blob).
        let (name_at, name_len) = match find(footer, &name_pattern) {
            Some(pos) => (pos, name_pattern.len() - 1),
            None => footer_name_slot(footer).ok_or_else(|| {
                Error(format!(
                    "could not find a preset name in the template footer (expected '{preset_name}')"
                ))
            })?,
        };

        Ok(Blob {
            head,
            post_len,
            prog,
            mid,
            comp_head,
            be_len_fields,
            le_len_fields,
            state,
            kind,
            comp_trailer,
            payload_tail,
            footer_prefix: footer[..name_at].to_vec(),
            footer_suffix: footer[name_at + name_len + 1..].to_vec(),
            comp_len_field,
            payload_len_field,
        })
    }

    /// Reassemble the blob around new plugin state and a new preset name,
    /// shifting every recorded length field by the change in state size.
    pub fn rebuild(&self, state: &[u8], name: &str) -> Vec<u8> {
        let delta = state.len() as i64 - self.state.len() as i64;
        let adjusted = |field: u32| (((field as i64) + delta) as u32).to_le_bytes();

        let mut comp_head = self.comp_head.clone();
        for &at in &self.be_len_fields {
            let old = u32::from_be_bytes(comp_head[at..at + 4].try_into().expect("len checked"));
            comp_head[at..at + 4].copy_from_slice(&(((old as i64) + delta) as u32).to_be_bytes());
        }
        for &at in &self.le_len_fields {
            let old = u32::from_le_bytes(comp_head[at..at + 4].try_into().expect("len checked"));
            comp_head[at..at + 4].copy_from_slice(&(((old as i64) + delta) as u32).to_le_bytes());
        }

        let mut out = Vec::with_capacity(self.head.len() + state.len() + 256);
        out.extend_from_slice(&self.head);
        out.extend_from_slice(&adjusted(self.payload_len_field));
        out.extend_from_slice(&self.post_len);
        out.extend_from_slice(&self.prog);
        out.extend_from_slice(&adjusted(self.comp_len_field));
        out.extend_from_slice(&self.mid);
        out.extend_from_slice(&comp_head);
        out.extend_from_slice(state);
        out.extend_from_slice(&self.comp_trailer);
        out.extend_from_slice(&self.payload_tail);
        out.extend_from_slice(&self.footer_prefix);
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.extend_from_slice(&self.footer_suffix);
        out
    }
}

fn read_u32(data: &[u8], off: usize) -> Result<u32> {
    data.get(off..off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().expect("len checked")))
        .ok_or_else(|| Error("template preset blob is truncated".into()))
}

fn slice4(data: &[u8], off: usize) -> Result<[u8; 4]> {
    data.get(off..off + 4)
        .map(|b| b.try_into().expect("len checked"))
        .ok_or_else(|| Error("template preset blob is truncated".into()))
}

/// Locate the embedded preset name structurally: the first run of
/// non-control bytes in the footer, which must be terminated by a null.
/// Returns (offset, length in bytes).
fn footer_name_slot(footer: &[u8]) -> Option<(usize, usize)> {
    let start = footer.iter().position(|&b| b >= 0x20)?;
    let len = footer[start..].iter().position(|&b| b < 0x20)?;
    if footer.get(start + len) == Some(&0) {
        Some((start, len))
    } else {
        None
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
