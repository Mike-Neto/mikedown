//! Inline text retains its original coordinates until references are resolved.

use alloc::collections::BTreeMap;
use core::slice;

use crate::{Diagnostic, parser};

/// Registered IDs mapped to list ordinal paths, or no path for table IDs.
pub type Targets = BTreeMap<String, Option<String>>;

/// Inline source text with its original diagnostic coordinates.
pub struct Text {
    /// One-based Unicode scalar column of the original text.
    pub column: usize,
    /// One-based source line of the original text.
    pub line: usize,
    /// Current spelling, updated when generated references are resolved.
    pub value: String,
}

impl Text {
    /// Create source-located text before resolving references.
    pub(crate) fn at(value: &str, line: usize, column: usize) -> Self {
        Self {
            value: value.into(),
            line,
            column,
        }
    }

    /// Resolve item links and validate target IDs using original source coordinates.
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

/// Resolve references across nonempty line groups while preserving paragraph boundaries.
pub fn resolve_lines(lines: &mut Vec<Text>, targets: &Targets) -> Result<(), Diagnostic> {
    let mut output = Vec::new();
    let mut start = 0;
    for (index, line) in lines.iter().enumerate() {
        if line.value.is_empty() {
            append_resolved(
                lines.split_at(index).0.split_at(start).1,
                targets,
                &mut output,
            )?;
            output.push(Text::at("", line.line, line.column));
            start = index.strict_add(1);
        }
    }
    append_resolved(lines.split_at(start).1, targets, &mut output)?;
    *lines = output;
    Ok(())
}

/// Append resolved lines, retaining available original line coordinates.
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

/// Resolve item links and validate target IDs using original source coordinates.
#[expect(
    clippy::too_many_lines,
    reason = "Reference recognition and replacement share original source coordinates"
)]
fn resolve(lines: &[Text], targets: &Targets) -> Result<String, Diagnostic> {
    let value = lines
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = String::new();
    let mut end = 0;
    for reference in {
        // Item references.
        use crate::inline::{ItemReference, balanced, protected_len};
        let text: &str = &value;

        let mut references = Vec::new();
        // Offsets advance only by complete recognized spans or Unicode scalars.
        let mut offset: usize = 0;
        while offset < text.len() {
            let tail = text.split_at(offset).1;
            if let Some(escaped) = tail.strip_prefix('\\') {
                offset = offset
                    .strict_add(1)
                    .strict_add(escaped.chars().next().map_or(0, char::len_utf8));
                continue;
            }
            if tail.starts_with('[')
                && let Some(label_end) = balanced(tail, '[', ']')
                && let Some(link_tail) = tail.get(label_end..)
                && link_tail.starts_with('(')
                && let Some(destination_end) = balanced(link_tail, '(', ')')
            {
                let link_end = label_end.strict_add(destination_end);
                let destination = tail
                    .split_at(link_end.strict_sub(1))
                    .0
                    .split_at(label_end.strict_add(1))
                    .1;
                if let Some(id) = destination.strip_prefix("item:") {
                    references.push(ItemReference {
                        start: offset,
                        end: offset.strict_add(link_end),
                        target_offset: offset
                            .strict_add(label_end)
                            .strict_add(1)
                            .strict_add("item:".len()),
                        id,
                    });
                }
                offset = offset.strict_add(link_end);
                continue;
            }
            if let Some(length) = protected_len(tail) {
                offset = offset.strict_add(length);
            } else {
                offset = offset.strict_add(tail.chars().next().map_or(1, char::len_utf8));
            }
        }
        references
    } {
        let mut offset = reference.target_offset;
        let mut location = (1, 1);
        for line in lines {
            if offset <= line.value.len() {
                location = (
                    line.line,
                    line.column
                        .strict_add(line.value.split_at(offset).0.chars().count()),
                );
                break;
            }
            offset = offset.strict_sub(line.value.len().strict_add(1));
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
        output.push_str(value.split_at(reference.start).0.split_at(end).1);
        output.push('[');
        output.push_str(label);
        output.push_str("](item:");
        output.push_str(reference.id);
        output.push(')');
        end = reference.end;
    }
    output.push_str(value.split_at(end).1);
    Ok(output)
}
