//! Mikedown's editor-independent, deterministic plain-text formatter.
//!
//! The formatted output is valid input: tables, annotations, and generated width
//! references are all parsed again on subsequent runs.

mod parser;
mod table;

use std::collections::{BTreeMap, BTreeSet};
use unicode_width::UnicodeWidthStr;

use parser::{Document, Kind, ListItem};
use table::{Alignment, Style};

/// A formatting error located in the original source (one-based coordinates).
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct Diagnostic {
    /// One-based source line.
    pub line: usize,
    /// One-based Unicode scalar column.
    pub column: usize,
    /// Human-readable explanation.
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for Diagnostic {}

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
}

impl Annotation {
    fn new(name: &str, line: usize) -> Self {
        Self {
            name: name.into(),
            options: BTreeMap::new(),
            line,
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
#[inline]
pub fn format(source: &str) -> Result<String, Vec<Diagnostic>> {
    let mut document = parser::parse(source).map_err(|diagnostic| vec![diagnostic])?;
    format_document(&mut document).map_err(|diagnostic| vec![diagnostic])
}

fn format_document(document: &mut Document) -> Result<String, Diagnostic> {
    let base_width = document
        .annotation
        .as_ref()
        .map_or(Some(80), |a| a.width(Some(80)));
    let default_style = document
        .annotation
        .as_ref()
        .and_then(|a| a.options.get("table"))
        .map_or(Style::Unicode, |s| {
            Style::parse(s).unwrap_or(Style::Unicode)
        });
    let TableLayout {
        ids,
        widest,
        mut text,
    } = measure_tables(document, default_style)?;

    let mut effective_width = base_width;
    if let (Some(minimum), Some((index, table_width))) = (base_width, widest) {
        if table_width > minimum {
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
            let annotation = document
                .annotation
                .get_or_insert_with(|| Annotation::new("document", 1));
            annotation
                .options
                .insert("wrap".into(), format!("table:{id}"));
            effective_width = Some(table_width);
        } else if let Some(annotation) = &mut document.annotation {
            annotation.options.remove("wrap");
        }
    } else if let Some(annotation) = &mut document.annotation {
        annotation.options.remove("wrap");
    }

    let mut pieces = Vec::new();
    if let Some(annotation) = &document.annotation {
        pieces.push(annotation.emit());
    }
    for (index, block) in document.blocks.iter().enumerate() {
        let width = block
            .annotation
            .as_ref()
            .map_or(effective_width, |a| a.width(effective_width));
        let body = match &block.kind {
            Kind::Paragraph(lines) => wrap_lines(lines, width, "", ""),
            Kind::Heading(level, text) => format!("{} {}", "#".repeat(*level), text.trim()),
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

struct TableLayout {
    ids: BTreeSet<String>,
    widest: Option<(usize, usize)>,
    text: BTreeMap<usize, String>,
}

fn measure_tables(
    document: &mut Document,
    default_style: Style,
) -> Result<TableLayout, Diagnostic> {
    let mut ids = BTreeSet::new();
    let mut widest: Option<(usize, usize)> = None;
    let mut table_text = BTreeMap::new();

    // Tables are independent of prose width, so this pass cannot form a layout cycle.
    for (index, block) in document.blocks.iter_mut().enumerate() {
        if let Some(id) = block.annotation.as_ref().and_then(|a| a.options.get("id"))
            && !ids.insert(id.clone())
        {
            return Err(error(block.line, format!("duplicate table ID '{id}'")));
        }
        if let Kind::Table(table) = &block.kind {
            if table.has_alignment()
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
            let style = block
                .annotation
                .as_ref()
                .and_then(|a| a.options.get("style"))
                .map_or(default_style, |s| Style::parse(s).unwrap_or(default_style));
            let alignment = block
                .annotation
                .as_ref()
                .and_then(|a| a.options.get("align"));
            let alignments =
                alignment.map(|a| a.split(',').map(Alignment::parse).collect::<Vec<_>>());
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

fn emit_quote(lines: &[String], width: Option<usize>) -> String {
    let mut output = Vec::new();
    let mut paragraph = Vec::new();
    for line in lines {
        if line.is_empty() {
            if !paragraph.is_empty() {
                output.push(wrap_lines(&paragraph, width, "> ", "> "));
                paragraph.clear();
            }
            output.push(">".into());
        } else {
            paragraph.push(line.clone());
        }
    }
    if !paragraph.is_empty() {
        output.push(wrap_lines(&paragraph, width, "> ", "> "));
    }
    output.join("\n")
}

fn emit_list(items: &[ListItem], annotation: Option<&Annotation>, width: Option<usize>) -> String {
    let root = items.first().map_or(0, |item| item.indent);
    let mut numbers: BTreeMap<usize, usize> = BTreeMap::new();
    let mut output = Vec::new();
    for item in items {
        numbers.retain(|indent, _| *indent <= item.indent);
        let at_root = item.indent == root;
        let ordered = if at_root {
            annotation.map_or(item.ordered, |a| a.name == "ol")
        } else {
            item.ordered
        };
        let indent = " ".repeat(item.indent - root);
        let marker = if ordered {
            let start = if at_root {
                annotation
                    .and_then(|a| a.options.get("start"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1)
            } else {
                1
            };
            let number = numbers.entry(item.indent).or_insert(start);
            let marker = format!("{number}.");
            *number = number.saturating_add(1);
            marker
        } else {
            numbers.remove(&item.indent);
            "-".into()
        };
        let prefix = format!("{indent}{marker} ");
        let continuation = " ".repeat(UnicodeWidthStr::width(prefix.as_str()));
        output.push(wrap_lines(&item.lines, width, &prefix, &continuation));
    }
    output.join("\n")
}

/// Width=off preserves physical line boundaries; wrapping never splits tokens or
/// balanced inline backtick spans (including links with spaces inside them).
fn wrap_lines(lines: &[String], width: Option<usize>, first: &str, rest: &str) -> String {
    let Some(limit) = width else {
        return lines
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{}{line}", if i == 0 { first } else { rest }))
            .collect::<Vec<_>>()
            .join("\n");
    };
    let text = lines.join(" ");
    let tokens = inline_tokens(&text);
    let mut output = Vec::new();
    let mut current = first.to_owned();
    let mut has_word = false;
    for token in tokens {
        let needed = UnicodeWidthStr::width(current.as_str())
            + usize::from(has_word)
            + UnicodeWidthStr::width(token);
        if has_word
            && needed > limit
            && !unsafe_line_start(token)
            && current.trim() != "---"
            && !ascii_rule(current.trim())
        {
            output.push(std::mem::take(&mut current));
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
fn unsafe_line_start(token: &str) -> bool {
    matches!(token, "-" | "+" | "*" | ">" | "---")
        || parser::is_annotation(token)
        || token.starts_with(['|', '│', '┌', '├', '└'])
        || token.starts_with("```")
        || token.starts_with("~~~")
        || ascii_rule(token)
        || ((1..=6).contains(&token.len()) && token.bytes().all(|ch| ch == b'#'))
        || token
            .strip_suffix('.')
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|ch| ch.is_ascii_digit()))
}

fn ascii_rule(text: &str) -> bool {
    text.starts_with('+') && text.contains('-') && text.chars().all(|ch| ch == '+' || ch == '-')
}

fn inline_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut fence = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '`' {
            start.get_or_insert(index);
            let mut count = 1;
            while chars.peek().is_some_and(|(_, next)| *next == '`') {
                chars.next();
                count += 1;
            }
            if fence == 0 {
                fence = count;
            } else if fence == count {
                fence = 0;
            }
        } else if ch.is_whitespace() && fence == 0 {
            if let Some(begin) = start.take() {
                tokens.push(&text[begin..index]);
            }
        } else {
            start.get_or_insert(index);
        }
    }
    if let Some(begin) = start {
        tokens.push(&text[begin..]);
    }
    tokens
}
