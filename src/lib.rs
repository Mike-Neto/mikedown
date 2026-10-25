//! Mikedown's editor-independent, deterministic plain-text formatter.
//!
//! The formatted output is valid input: tables, annotations, and generated width
//! references are all parsed again on subsequent runs.

extern crate alloc;

mod inline;
mod list;
mod parser;
mod table;
mod text;

use alloc::collections::{BTreeMap, BTreeSet};
use core::mem;
use unicode_width::UnicodeWidthStr;

use parser::{Document, Kind, ListItem};
use table::{Alignment, Style};
use text::{Targets, Text};

const DOCUMENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A formatting error located in the original source (one-based coordinates).
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{line}:{column}: {message}")]
#[non_exhaustive]
pub struct Diagnostic {
    /// One-based source line.
    pub line: usize,
    /// One-based Unicode scalar column.
    pub column: usize,
    /// Human-readable explanation.
    pub message: String,
}

fn error(line: usize, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        line,
        column: 1,
        message: message.into(),
    }
}

#[derive(Clone, Debug)]
struct Annotation {
    name: String,
    options: BTreeMap<String, String>,
    line: usize,
    id_column: usize,
}

impl Annotation {
    fn new(name: &str, line: usize) -> Self {
        Self {
            name: name.into(),
            options: BTreeMap::new(),
            line,
            id_column: 1,
        }
    }

    fn emit(&self) -> String {
        let mut text = format!("@{}", self.name);
        for (key, value) in &self.options {
            text.push(' ');
            text.push_str(key);
            text.push('=');
            text.push_str(value);
        }
        text
    }

    fn width(&self, inherited: Option<usize>) -> Option<usize> {
        match self.options.get("width").map(String::as_str) {
            Some("off") => None,
            Some(value) => value.parse().ok(),
            None => inherited,
        }
    }
}

/// Format an entire UTF-8 Mikedown document without accessing the filesystem.
///
/// # Errors
/// Returns source-located diagnostics for malformed or unsupported input. No
/// partial output is returned. Unknown inline syntax remains ordinary text.
/// Document annotations and version declarations are optional and preserved when supplied.
#[inline]
pub fn format(source: &str) -> Result<String, Vec<Diagnostic>> {
    let mut document = parser::parse(source).map_err(|diagnostic| vec![diagnostic])?;
    format_document(&mut document).map_err(|diagnostic| vec![diagnostic])
}

fn format_document(document: &mut Document) -> Result<String, Diagnostic> {
    let targets = prepare_references(document)?;
    let reserved_ids = targets.keys().cloned().collect();
    let base_width = document
        .annotation
        .as_ref()
        .map_or(Some(80), |a| a.width(Some(80)));
    let default_style = document
        .annotation
        .as_ref()
        .and_then(|a| a.options.get("table"))
        .map_or(Style::Pipes, |s| Style::parse(s).unwrap_or(Style::Pipes));
    let TableLayout {
        ids,
        widest,
        mut text,
    } = measure_tables(document, default_style, reserved_ids)?;

    let wrapping_table =
        widest.filter(|(_, width)| base_width.is_some_and(|minimum| *width > minimum));
    let effective_width = wrapping_table.map(|(_, width)| width).or(base_width);
    if let Some(document_annotation) = &mut document.annotation {
        if let Some((index, _)) = wrapping_table {
            let block = &mut document.blocks[index];
            let annotation = block
                .annotation
                .get_or_insert_with(|| Annotation::new("table", block.line));
            let id = if let Some(id) = annotation.options.get("id") {
                id.clone()
            } else {
                let mut suffix = 1;
                while ids.contains(&format!("table-{suffix}")) {
                    suffix += 1;
                }
                let id = format!("table-{suffix}");
                annotation.options.insert("id".into(), id.clone());
                id
            };
            document_annotation
                .options
                .insert("wrap".into(), format!("table:{id}"));
        } else {
            document_annotation.options.remove("wrap");
        }
    }

    let mut pieces = Vec::new();
    if let Some(front_matter) = &document.front_matter {
        pieces.push(front_matter.clone());
    }
    if let Some(annotation) = &document.annotation {
        pieces.push(annotation.emit());
    }
    for (index, block) in document.blocks.iter().enumerate() {
        let width = block
            .annotation
            .as_ref()
            .map_or(effective_width, |a| a.width(effective_width));
        let body = match &block.kind {
            Kind::Paragraph(lines) => wrap_lines(lines, width, "", "", false),
            Kind::Heading(level, text) => format!("{} {}", "#".repeat(*level), text.value.trim()),
            Kind::Quote(lines) => emit_quote(lines, width),
            Kind::List(items) => emit_list(items, block.annotation.as_ref(), width),
            Kind::Table(_) => text.remove(&index).unwrap_or_default(),
            Kind::Literal(text) => text.clone(),
        };
        if let Some(annotation) = &block.annotation {
            pieces.push(format!("{}\n{body}", annotation.emit()));
        } else {
            pieces.push(body);
        }
    }
    if pieces.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("{}\n", pieces.join("\n\n")))
    }
}

fn register_target(
    targets: &mut Targets,
    id: &str,
    path: Option<String>,
    annotation: &Annotation,
) -> Result<(), Diagnostic> {
    if let Some(previous) = targets.get(id) {
        let message = if previous.is_none() && path.is_none() {
            format!("duplicate table ID '{id}'")
        } else {
            format!("duplicate ID '{id}'")
        };
        return Err(Diagnostic {
            line: annotation.line,
            column: annotation.id_column,
            message,
        });
    }
    targets.insert(id.into(), path);
    Ok(())
}

fn prepare_references(document: &mut Document) -> Result<Targets, Diagnostic> {
    let mut targets = Targets::new();
    for block in &mut document.blocks {
        if let Some(annotation) = &block.annotation
            && let Some(id) = annotation.options.get("id")
        {
            register_target(&mut targets, id, None, annotation)?;
        }
        if let Kind::List(items) = &mut block.kind {
            list::assign(items, block.annotation.as_ref());
            for item in items {
                if let Some(anchor) = &item.anchor
                    && let Some(id) = anchor.options.get("id")
                {
                    register_target(&mut targets, id, Some(item.path.clone()), anchor)?;
                }
            }
        }
    }
    for block in &mut document.blocks {
        match &mut block.kind {
            Kind::Paragraph(lines) | Kind::Quote(lines) => text::resolve_lines(lines, &targets)?,
            Kind::Heading(_, value) => value.resolve(&targets)?,
            Kind::List(items) => {
                for item in items {
                    text::resolve_lines(&mut item.lines, &targets)?;
                }
            }
            Kind::Table(table) => table.resolve_references(&targets)?,
            Kind::Literal(_) => {}
        }
    }
    Ok(targets)
}

struct TableLayout {
    ids: BTreeSet<String>,
    widest: Option<(usize, usize)>,
    text: BTreeMap<usize, String>,
}

fn measure_tables(
    document: &mut Document,
    default_style: Style,
    ids: BTreeSet<String>,
) -> Result<TableLayout, Diagnostic> {
    let mut widest: Option<(usize, usize)> = None;
    let mut table_text = BTreeMap::new();

    // Tables are independent of prose width, so this pass cannot form a layout cycle.
    for (index, block) in document.blocks.iter_mut().enumerate() {
        if let Kind::Table(table) = &block.kind {
            let style = block
                .annotation
                .as_ref()
                .and_then(|a| a.options.get("style"))
                .map_or(default_style, |s| Style::parse(s).unwrap_or(default_style));
            if style != Style::Pipes
                && table.has_alignment()
                && block
                    .annotation
                    .as_ref()
                    .is_none_or(|a| !a.options.contains_key("align"))
            {
                let annotation = block
                    .annotation
                    .get_or_insert_with(|| Annotation::new("table", block.line));
                annotation
                    .options
                    .insert("align".into(), table.alignment_option());
            }
            let alignment = block
                .annotation
                .as_ref()
                .and_then(|a| a.options.get("align"));
            let alignments = alignment.and_then(|a| Alignment::parse_list(a));
            if let Some(alignments) = &alignments
                && alignments.len() != table.columns()
            {
                return Err(error(
                    block.line,
                    "align must contain one alignment per table column",
                ));
            }
            let text = table.emit(style, alignments.as_deref());
            let width = text.lines().map(UnicodeWidthStr::width).max().unwrap_or(0);
            if widest.is_none_or(|(_, previous)| width > previous) {
                widest = Some((index, width));
            }
            table_text.insert(index, text);
        }
    }

    Ok(TableLayout {
        ids,
        widest,
        text: table_text,
    })
}

fn emit_quote(lines: &[Text], width: Option<usize>) -> String {
    let mut output = Vec::new();
    let mut paragraph = Vec::new();
    for line in lines {
        if line.value.is_empty() {
            if !paragraph.is_empty() {
                output.push(wrap_lines(&paragraph, width, "> ", "> ", false));
                paragraph.clear();
            }
            output.push(">".into());
        } else {
            paragraph.push(line.value.clone());
        }
    }
    if !paragraph.is_empty() {
        output.push(wrap_lines(&paragraph, width, "> ", "> ", false));
    }
    output.join("\n")
}

fn emit_list(items: &[ListItem], annotation: Option<&Annotation>, width: Option<usize>) -> String {
    let root = items.first().map_or(0, |item| item.indent);
    let cycle = annotation.is_some_and(|value| value.name == "ol");
    let checklist = cycle && items.iter().any(|item| item.checkbox.is_some());
    let mut output = Vec::new();
    for item in items {
        let indent = " ".repeat(item.indent - root);
        if let Some(anchor) = &item.anchor {
            output.push(format!("{indent}{}", anchor.emit()));
        }
        let wrapper = if checklist { "- " } else { "" };
        let prefix = item.checkbox.map_or_else(
            || format!("{indent}{wrapper}{} ", item.marker),
            |state| format!("{indent}{wrapper}[{state}] {} ", item.marker),
        );
        let continuation = " ".repeat(UnicodeWidthStr::width(prefix.as_str()));
        output.push(wrap_lines(
            &item.lines,
            width,
            &prefix,
            &continuation,
            cycle,
        ));
    }
    output.join("\n")
}

/// Width=off preserves physical line boundaries; wrapping never splits tokens or
/// balanced inline backtick spans (including links with spaces inside them).
fn wrap_lines(
    lines: &[impl AsRef<str>],
    width: Option<usize>,
    first: &str,
    rest: &str,
    cycle: bool,
) -> String {
    let Some(limit) = width else {
        return lines
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{}{}", if i == 0 { first } else { rest }, line.as_ref()))
            .collect::<Vec<_>>()
            .join("\n");
    };
    if lines.iter().any(|line| hard_break(line.as_ref()).is_some()) {
        let mut output = Vec::new();
        let mut segment = Vec::new();
        for line in lines {
            let line = line.as_ref();
            let marker = hard_break(line);
            segment.push(marker.map_or_else(|| line.to_owned(), |(body, _)| body.to_owned()));
            if let Some((_, suffix)) = marker {
                let prefix = if output.is_empty() { first } else { rest };
                output.push(format!(
                    "{}{suffix}",
                    wrap_lines(&segment, width, prefix, rest, cycle)
                ));
                segment.clear();
            }
        }
        if !segment.is_empty() {
            output.push(wrap_lines(
                &segment,
                width,
                if output.is_empty() { first } else { rest },
                rest,
                cycle,
            ));
        }
        return output.join("\n");
    }
    let text = lines
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(" ");
    let tokens = inline::tokens(&text);
    let mut output = Vec::new();
    let mut current = first.to_owned();
    let mut has_word = false;
    for token in tokens {
        let needed = UnicodeWidthStr::width(current.as_str())
            + usize::from(has_word)
            + UnicodeWidthStr::width(token);
        if has_word
            && needed > limit
            && !unsafe_line_start(token, cycle)
            && current.trim() != "---"
            && !ascii_rule(current.trim())
        {
            output.push(mem::take(&mut current));
            current.push_str(rest);
            has_word = false;
        }
        if has_word {
            current.push(' ');
        }
        current.push_str(token);
        has_word = true;
    }
    if has_word {
        output.push(current);
    }
    if output.is_empty() {
        first.trim_end().into()
    } else {
        output.join("\n")
    }
}

// A wrap must not turn inline text into a heading, list, annotation, fence, or
// table when the saved output is parsed again. Such tokens stay on the old line.
fn unsafe_line_start(token: &str, cycle: bool) -> bool {
    parser::starts_block(token) || (cycle && parser::cycle_marker(token).is_some())
}

fn ascii_rule(text: &str) -> bool {
    inline::ascii_rule(text).is_ok()
}

#[expect(
    clippy::integer_division_remainder_used,
    reason = "Backslash parity uses public document text and needs no constant-time arithmetic"
)]
fn hard_break(line: &str) -> Option<(&str, &str)> {
    if line.ends_with("  ") {
        return Some((line.trim_end(), &line[line.trim_end().len()..]));
    }
    let slashes = line.chars().rev().take_while(|c| *c == '\\').count();
    (slashes % 2 == 1).then(|| (&line[..line.len() - 1], "\\"))
}
