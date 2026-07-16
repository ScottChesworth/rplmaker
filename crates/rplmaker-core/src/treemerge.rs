//! Overlay parameter values from a parsed preset file onto a template
//! plugin state that is itself a JUCE ValueTree (older JUCE plugins, e.g.
//! Archetype Gojira).
//!
//! Mirrors xmlmerge: the template tree provides the complete structure and
//! defaults; only values whose property exists on the matching source node
//! are replaced. Children are matched by their "id" property when they have
//! one — JUCE's AudioProcessorValueTreeState stores every parameter as a
//! node called PARAM distinguished only by id — and otherwise by ordinal
//! position among same-named siblings, which keeps repeated nodes like
//! Gojira's two "cabsim" entries paired up correctly.

use crate::valuetree::{self, find_node, Node, Value};
use crate::xmlmerge::MergeStats;
use crate::{Error, Result};
use std::collections::HashMap;

pub fn merge(template_root: &Node, source: &Node) -> Result<(Vec<u8>, MergeStats)> {
    let matched = find_node(source, &template_root.name).ok_or_else(|| {
        Error(format!(
            "the preset file contains no '{}' data matching this plugin's state",
            template_root.name
        ))
    })?;
    let mut stats = MergeStats { overridden: 0, template_only: 0 };
    let merged = overlay(template_root, matched, &mut stats);
    Ok((valuetree::write(&merged), stats))
}

fn overlay(template: &Node, source: &Node, stats: &mut MergeStats) -> Node {
    let props = template
        .props
        .iter()
        .map(|(name, template_value)| match source.prop(name) {
            Some(source_value) => {
                stats.overridden += 1;
                (name.clone(), coerce(template_value, source_value))
            }
            None => {
                stats.template_only += 1;
                (name.clone(), template_value.clone())
            }
        })
        .collect();
    let mut same_name_seen: HashMap<&str, usize> = HashMap::new();
    let children = template
        .children
        .iter()
        .map(|template_child| {
            let ordinal = same_name_seen.entry(template_child.name.as_str()).or_insert(0);
            let matched = matching_child(source, template_child, *ordinal);
            *ordinal += 1;
            match matched {
                Some(source_child) => overlay(template_child, source_child, stats),
                None => template_child.clone(),
            }
        })
        .collect();
    Node {
        name: template.name.clone(),
        props,
        children,
    }
}

/// `ordinal` is the template child's position among its same-named siblings,
/// used when there is no id to match on.
fn matching_child<'a>(source: &'a Node, template_child: &Node, ordinal: usize) -> Option<&'a Node> {
    let same_named = source
        .children
        .iter()
        .filter(|c| c.name == template_child.name);
    match template_child.prop("id").map(Value::as_text) {
        Some(id) => {
            let mut same_named = same_named;
            same_named.find(|c| c.prop("id").map(Value::as_text).as_deref() == Some(id.as_str()))
        }
        None => same_named.into_iter().nth(ordinal),
    }
}

/// Convert the source value to the template value's type, so text values
/// from XML preset files land as the typed values the plugin state uses.
/// If conversion fails, the template's value is kept.
fn coerce(template_value: &Value, source_value: &Value) -> Value {
    use Value::*;
    match (template_value, source_value) {
        (Int(_), Int(_))
        | (Int64(_), Int64(_))
        | (Double(_), Double(_))
        | (Bool(_), Bool(_))
        | (Str(_), Str(_)) => source_value.clone(),
        (Str(_), other) => Str(other.as_text()),
        (Int(t), other) => other.as_text().parse().map(Int).unwrap_or(Int(*t)),
        (Int64(t), other) => other.as_text().parse().map(Int64).unwrap_or(Int64(*t)),
        (Double(t), other) => other.as_text().parse().map(Double).unwrap_or(Double(*t)),
        (Bool(t), other) => match other.as_text().as_str() {
            "true" | "1" => Bool(true),
            "false" | "0" => Bool(false),
            _ => Bool(*t),
        },
        (template_value, _) => template_value.clone(),
    }
}
