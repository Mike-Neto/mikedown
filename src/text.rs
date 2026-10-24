//! Inline text retains its original coordinates until references are resolved.

use alloc::collections::BTreeMap;
use core::slice;

use crate::{Diagnostic, inline, parser};

pub(crate) type Targets = BTreeMap<String, Option<String>>;

pub(crate) struct Text {
    pub value: String,
    pub line: usize,
    pub column: usize,
}

impl Text {
    pub(crate) fn at(value: &str, line: usize, column: usize) -> Self {
        Self {
            value: value.into(),
            line,
            column,
        }
    }

    pub(crate) fn resolve(&mut self, targets: &Targets) -> Result<(), Diagnostic> {
        self.value = resolve(slice::from_ref(self), targets)?;
        Ok(())
    }
}

impl AsRef<str> for Text {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

pub(crate) fn resolve_lines(lines: &mut Vec<Text>, targets: &Targets) -> Result<(), Diagnostic> {
    let mut output = Vec::new();
    let mut start = 0;
    for (index, line) in lines.iter().enumerate() {
        if line.value.is_empty() {
            append_resolved(&lines[start..index], targets, &mut output)?;
            output.push(Text::at("", line.line, line.column));
            start = index + 1;
        }
    }
    append_resolved(&lines[start..], targets, &mut output)?;
    *lines = output;
    Ok(())
}

fn append_resolved(
    lines: &[Text],
    targets: &Targets,
    output: &mut Vec<Text>,
) -> Result<(), Diagnostic> {
    if let Some(first) = lines.first() {
        let value = resolve(lines, targets)?;
        for (index, part) in value.split('\n').enumerate() {
            let original = lines.get(index).unwrap_or(first);
            output.push(Text::at(part, original.line, original.column));
        }
    }
    Ok(())
}

fn resolve(lines: &[Text], targets: &Targets) -> Result<String, Diagnostic> {
    let value = lines
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = String::new();
    let mut end = 0;
    for reference in inline::item_references(&value) {
        let mut offset = reference.target_offset;
        let mut location = (1, 1);
        for line in lines {
            if offset <= line.value.len() {
                location = (
                    line.line,
                    line.column + line.value[..offset].chars().count(),
                );
                break;
            }
            offset -= line.value.len() + 1;
        }
        let failure = |message| Diagnostic {
            line: location.0,
            column: location.1,
            message,
        };
        if !parser::valid_id(reference.id) {
            return Err(failure(format!(
                "invalid item reference ID '{}'",
                reference.id
            )));
        }
        let label = match targets.get(reference.id) {
            Some(Some(label)) => label,
            Some(None) => {
                return Err(failure(format!(
                    "item reference '{}' targets a table",
                    reference.id
                )));
            }
            None => {
                return Err(failure(format!(
                    "unknown item reference '{}'",
                    reference.id
                )));
            }
        };
        output.push_str(&value[end..reference.start]);
        output.push('[');
        output.push_str(label);
        output.push_str("](item:");
        output.push_str(reference.id);
        output.push(')');
        end = reference.end;
    }
    output.push_str(&value[end..]);
    Ok(output)
}
