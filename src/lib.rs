//! Mikedown's editor-independent, deterministic plain-text formatter.
//!
//! The formatted output is valid input: tables, annotations, and generated width
//! references are all parsed again on subsequent runs.

extern crate alloc;

mod inline;
mod list;
/// Block recognizers and source-located syntax validation.
mod parser;
/// Table recognition, alignment, and border rendering.
mod table;
mod text;

use alloc::collections::{BTreeMap, BTreeSet};
use core::mem;
use unicode_width::UnicodeWidthStr;

use parser::{Document, Kind};
use table::{Alignment, Style};
use text::{Targets, Text};

/// Language version accepted by explicit document declarations.
const DOCUMENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A formatting error located in the original source (one-based coordinates).
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{line}:{column}: {message}")]
#[non_exhaustive]
pub struct Diagnostic {
    /// One-based Unicode scalar column.
    pub column: usize,
    /// One-based source line.
    pub line: usize,
    /// Human-readable explanation.
    pub message: String,
}

/// Validated settings with coordinates for source diagnostics.
#[derive(Clone, Debug)]
struct Annotation {
    /// One-based scalar column of the ID option.
    id_column: usize,
    /// One-based source line of the annotation.
    line: usize,
    /// Recognized annotation name without its at sign.
    name: String,
    /// Validated option values, sorted for deterministic emission.
    options: BTreeMap<String, String>,
}

impl Annotation {
    /// Render the annotation with options in canonical order.
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

    /// Create an annotation with no options at the supplied source line.
    fn new(name: &str, line: usize) -> Self {
        Self {
            name: name.into(),
            options: BTreeMap::new(),
            line,
            id_column: 1,
        }
    }

    /// Resolve local wrapping width, including explicit width=off.
    fn width(&self, inherited: Option<usize>) -> Option<usize> {
        match self.options.get("width").map(String::as_str) {
            Some("off") => None,
            Some(value) => value.parse().ok(),
            None => inherited,
        }
    }
}

/// Measured table output and IDs used to choose document wrapping width.
struct TableLayout {
    /// Reserved table and item IDs, including generated table IDs.
    ids: BTreeSet<String>,
    /// Rendered table bodies keyed by their document block indices.
    text: BTreeMap<usize, String>,
    /// Index and display width of the widest rendered table, if any.
    widest: Option<(usize, usize)>,
}

/// Create a diagnostic at the first column of a source line.
fn error(line: usize, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        line,
        column: 1,
        message: message.into(),
    }
}

/// Format an entire UTF-8 Mikedown document without accessing the filesystem.
///
/// # Errors
/// Returns source-located diagnostics for malformed or unsupported input. No
/// partial output is returned. Unknown inline syntax remains ordinary text.
/// Document annotations and version declarations are optional and preserved when supplied.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "Single-use parsing and formatting passes stay inline in scoped stages"
)]
#[inline]
pub fn format(source: &str) -> Result<String, Vec<Diagnostic>> {
    let mut parsed_document = {
        // Parse.
        use crate::parser::{
            Block, HTML_BLOCK_TAGS, cycle_marker, fence, heading, html_start, indented, is_annotation, is_quote, is_rule, item_annotation, list_marker, located, parse_annotation, parse_list, quote,
            reference, setext, starts_block, table_at, whitespace,
        };
        use crate::table::Table;
        use nom::branch::alt;
        use nom::bytes::complete::{take_while, take_while1};
        use nom::character::complete::{char, one_of};
        use nom::combinator::{all_consuming, map_res, recognize, rest, verify};
        use nom::sequence::{delimited, preceded, terminated};
        use nom::{IResult, Parser as _};

        let result: Result<Document, Diagnostic> = 'parse: {
            let source_lines: Vec<&str> = source.lines().collect();
            let lines = &source_lines;
            let mut document = Document {
                annotation: None,
                front_matter: None,
                blocks: Vec::new(),
            };
            // The cursor advances at most to lines.len(); source line numbers are one-based.
            let mut index: usize = 0;
            if lines.first() == Some(&"---") {
                index = 1;
                while lines.get(index).is_some_and(|line| !matches!(*line, "---" | "...")) {
                    index = index.strict_add(1);
                }
                if index < lines.len() {
                    index = index.strict_add(1);
                }
                document.front_matter = Some(lines.split_at(index).0.join("\n"));
            }
            let mut pending: Option<Annotation> = None;
            while let Some(&line) = lines.get(index) {
                if line.trim().is_empty() {
                    index = index.strict_add(1);
                    continue;
                }
                let cycle = pending.as_ref().is_some_and(|annotation| annotation.name == "ol");
                if !indented(line) && item_annotation(line) && !cycle {
                    break 'parse Err(error(index.strict_add(1), "@item is only valid inside an @ol list"));
                }
                if !indented(line) && is_annotation(line) && !(cycle && item_annotation(line)) {
                    let annotation = match parse_annotation(line, index.strict_add(1)) {
                        Ok(value) => value,
                        Err(failure) => break 'parse Err(failure),
                    };
                    if let Some(previous) = pending.take() {
                        break 'parse Err(error(previous.line, "annotation has no target block"));
                    }
                    if annotation.name == "document" {
                        if document.annotation.is_some() || !document.blocks.is_empty() {
                            break 'parse Err(error(index.strict_add(1), "document annotation must occur once, before all blocks"));
                        }
                        document.annotation = Some(annotation);
                    } else {
                        pending = Some(annotation);
                    }
                    index = index.strict_add(1);
                    continue;
                }
                let start = index;
                let kind = {
                    let stage_result = {
                        // Parse block.

                        let block_index = &mut index;
                        let strict_table: bool = pending.as_ref().is_some_and(|annotation| annotation.name == "table");
                        let result: Result<Kind, Diagnostic> = 'parse_block: {
                            if cycle && !is_rule(line) && (cycle_marker(line).is_some() || item_annotation(line)) {
                                break 'parse_block parse_list(lines, block_index, true);
                            }
                            if indented(line) || html_start(line).is_some() || reference(line) {
                                break 'parse_block Ok({
                                    // Parse literal.

                                    let end = html_start(line);
                                    *block_index = block_index.strict_add(1);
                                    if indented(line) {
                                        while lines.get(*block_index).is_some_and(|next| indented(next) || next.trim().is_empty()) {
                                            *block_index = block_index.strict_add(1);
                                        }
                                        while *block_index > start.strict_add(1) && lines.get(block_index.strict_sub(1)).is_some_and(|previous| previous.trim().is_empty()) {
                                            *block_index = block_index.strict_sub(1);
                                        }
                                    } else if let Some(closer) = end {
                                        while !closer.is_empty() && lines.get(block_index.strict_sub(1)).is_some_and(|previous| !previous.to_ascii_lowercase().contains(closer)) && *block_index < lines.len() {
                                            *block_index = block_index.strict_add(1);
                                        }
                                        if closer.is_empty() {
                                            while lines.get(*block_index).is_some_and(|next| !next.is_empty()) {
                                                *block_index = block_index.strict_add(1);
                                            }
                                        }
                                    } else {
                                        while lines.get(*block_index).is_some_and(|next| indented(next)) {
                                            *block_index = block_index.strict_add(1);
                                        }
                                    }
                                    Kind::Literal(lines.split_at(*block_index).0.split_at(start).1.join("\n"))
                                });
                            }
                            if let Some((character, count)) = fence(line) {
                                break 'parse_block ({
                                    // Parse fence.

                                    let result: Result<Kind, Diagnostic> = 'parse_fence: {
                                        *block_index = block_index.strict_add(1);
                                        while let Some(&fence_line) = lines.get(*block_index) {
                                            if {
                                                // Closing fence.

                                                let input: &str = fence_line;
                                                let result: IResult<&str, &str> = {
                                                    all_consuming(preceded(
                                                        whitespace,
                                                        terminated(verify(take_while1(|ch| ch == character), |marks: &str| marks.len() >= count), whitespace),
                                                    ))
                                                    .parse(input)
                                                };
                                                result
                                            }
                                            .is_ok()
                                            {
                                                *block_index = block_index.strict_add(1);
                                                break 'parse_fence Ok(Kind::Literal(lines.split_at(*block_index).0.split_at(start).1.join("\n")));
                                            }
                                            *block_index = block_index.strict_add(1);
                                        }
                                        Err(error(start.strict_add(1), "unclosed code fence"))
                                    };
                                    result
                                });
                            }
                            if let Some((level, text)) = heading(line) {
                                *block_index = block_index.strict_add(1);
                                break 'parse_block Ok(Kind::Heading(level, located(text, line, start.strict_add(1))));
                            }
                            if is_rule(line) {
                                *block_index = block_index.strict_add(1);
                                break 'parse_block Ok(Kind::Literal(line.into()));
                            }
                            if is_quote(line) {
                                let mut body = Vec::new();
                                while let Some(&quote_line) = lines.get(*block_index).filter(|next| is_quote(next)) {
                                    let content = quote(quote_line).map_or("", |(_, content)| content);
                                    body.push(located(content.strip_prefix(' ').unwrap_or(content), quote_line, block_index.strict_add(1)));
                                    *block_index = block_index.strict_add(1);
                                }
                                break 'parse_block Ok(Kind::Quote(body));
                            }
                            if list_marker(line).is_some() {
                                break 'parse_block parse_list(lines, block_index, false);
                            }
                            if Table::starts(line) || table_at(lines, start) {
                                let gfm = table_at(lines, start);
                                *block_index = block_index.strict_add(1);
                                while lines.get(*block_index).is_some_and(|next| !next.trim().is_empty() && !is_annotation(next) && (Table::starts(next) || next.contains('|')))
                                {
                                    *block_index = block_index.strict_add(1);
                                }
                                let body = lines.split_at(*block_index).0.split_at(start).1;
                                let normalized: Vec<String> = body
                                    .iter()
                                    .map(|source_row| {
                                        let text = source_row.trim();
                                        if gfm {
                                            format!(
                                                "|{}|",
                                                text.strip_prefix('|').unwrap_or(text).strip_suffix('|').unwrap_or_else(|| text.strip_prefix('|').unwrap_or(text))
                                            )
                                        } else {
                                            (*source_row).to_owned()
                                        }
                                    })
                                    .collect();
                                let normalized_rows: Vec<&str> = normalized.iter().map(String::as_str).collect();
                                break 'parse_block ({
                                    let stage_result = {
                                        // Parse.
                                        use crate::table::separator_cell;
                                        let table_lines: &[&str] = &normalized_rows;
                                        let originals: &[&str] = body;
                                        let start = start.strict_add(1);
                                        let result: Result<Table, Diagnostic> = 'parse_table: {
                                            let mut rows: Vec<Vec<Text>> = Vec::new();
                                            let mut alignments = Vec::new();
                                            let mut header_boundary = false;
                                            for (offset, (row_line, original)) in table_lines.iter().zip(originals).enumerate() {
                                                let row_text = row_line.trim();
                                                if {
                                                    // Ascii border.

                                                    inline::ascii_rule(row_text).is_ok()
                                                } || row_text.starts_with(['┌', '├', '└'])
                                                {
                                                    {
                                                        let stage_result = {
                                                            // Validate border.

                                                            let border_line = start.strict_add(offset);
                                                            let result: Result<(), Diagnostic> = {
                                                                {
                                                                    // Border.

                                                                    let input: &str = row_text;
                                                                    let result: IResult<&str, &str> = {
                                                                        all_consuming(alt((
                                                                            verify(inline::ascii_rule, |border: &str| border.ends_with('+')),
                                                                            recognize(delimited(char('┌'), take_while(|ch| ch == '─' || ch == '┬'), char('┐'))),
                                                                            recognize(delimited(char('├'), take_while(|ch| ch == '─' || ch == '┼'), char('┤'))),
                                                                            recognize(delimited(char('└'), take_while(|ch| ch == '─' || ch == '┴'), char('┘'))),
                                                                        )))
                                                                        .parse(input)
                                                                    };
                                                                    result
                                                                }
                                                                .map(|_| ())
                                                                .map_err(|_parse_error| error(border_line, "malformed table border"))
                                                            };
                                                            result
                                                        };
                                                        match stage_result {
                                                            Ok(value) => value,
                                                            Err(failure) => break 'parse_table Err(failure),
                                                        }
                                                    }
                                                    continue;
                                                }
                                                let parsed = {
                                                    // Row.

                                                    let input: &str = row_text;
                                                    let result: IResult<&str, (char, &str)> = 'row: {
                                                        let (input, delimiter) = match one_of("|│").parse(input) {
                                                            Ok(value) => value,
                                                            Err(failure) => break 'row Err(failure),
                                                        };
                                                        let (input, row_body) = match map_res(rest, |row_body: &str| row_body.strip_suffix(delimiter).ok_or(())).parse(input) {
                                                            Ok(value) => value,
                                                            Err(failure) => break 'row Err(failure),
                                                        };
                                                        Ok((input, (delimiter, row_body)))
                                                    };
                                                    result
                                                }
                                                .or_else(|err| {
                                                    if row_text.contains('|') && !row_text.starts_with(['│', '|']) {
                                                        Ok(("", ('|', row_text.trim_start_matches('|').trim_end_matches('|'))))
                                                    } else {
                                                        Err(err)
                                                    }
                                                });
                                                let (_, (delimiter, row_body)) = match parsed.map_err(|_parse_error| error(start.strict_add(offset), "table rows must have opening and closing delimiters")) {
                                                    Ok(value) => value,
                                                    Err(failure) => break 'parse_table Err(failure),
                                                };
                                                let cells = inline::cells(row_body, delimiter);
                                                let separator = delimiter == '|' && cells.iter().all(|cell| separator_cell(cell).is_ok());
                                                if let Some(previous) = rows.first()
                                                    && cells.len() != previous.len()
                                                {
                                                    break 'parse_table Err(error(start.strict_add(offset), "inconsistent table column count"));
                                                }
                                                if separator {
                                                    if rows.len() != 1 || header_boundary {
                                                        break 'parse_table Err(error(start.strict_add(offset), "table separator must follow the header"));
                                                    }
                                                    alignments = cells
                                                        .iter()
                                                        .map(|cell| {
                                                            if cell.starts_with(':') && cell.ends_with(':') {
                                                                Alignment::Center
                                                            } else if cell.ends_with(':') {
                                                                Alignment::Right
                                                            } else {
                                                                Alignment::Left
                                                            }
                                                        })
                                                        .collect();
                                                    header_boundary = true;
                                                } else {
                                                                                                        let mut cursor: usize = 0;
                                                    rows.push(
                                                        cells
                                                            .into_iter()
                                                            .map(|cell| {
                                                                let begin = cursor.strict_add(original.split_at(cursor).1.find(cell).unwrap_or(0));
                                                                cursor = begin.strict_add(cell.len());
                                                                Text::at(cell, start.strict_add(offset), original.split_at(begin).0.chars().count().strict_add(1))
                                                            })
                                                            .collect(),
                                                    );
                                                }
                                            }
                                            if rows.is_empty() {
                                                break 'parse_table Err(error(start, "table has no header row"));
                                            }
                                            if alignments.is_empty() {
                                                alignments = vec![Alignment::Left; rows.first().map_or(0, Vec::len)];
                                            }
                                            Ok(Table { alignments, rows })
                                        };
                                        result
                                    };
                                    match stage_result {
                                        Ok(table) => Ok(Kind::Table(table)),
                                        Err(diagnostic) if strict_table => Err(diagnostic),
                                        Err(_) => Ok(Kind::Literal(body.join("\n"))),
                                    }
                                });
                            }
                            let mut body = vec![located(line.trim_start(), line, start.strict_add(1))];
                            *block_index = block_index.strict_add(1);
                            while let Some(&next) = lines.get(*block_index).filter(|next| !next.trim().is_empty()) {
                                if setext(next) {
                                    *block_index = block_index.strict_add(1);
                                    break 'parse_block Ok(Kind::Literal(lines.split_at(*block_index).0.split_at(start).1.join("\n")));
                                }
                                let generic_html = html_start(next) == Some("")
                                    && !{
                                        // Html block tag.


                                        let lower = next.trim_start().to_ascii_lowercase();
                                        let tail = lower.trim_start_matches('<').trim_start_matches('/');
                                        let name = tail.split(|character: char| !character.is_ascii_alphanumeric()).next().unwrap_or("");
                                        HTML_BLOCK_TAGS.contains(&name)
                                    };
                                if !indented(next) && ((!generic_html && starts_block(next)) || table_at(lines, *block_index)) {
                                    break;
                                }
                                body.push(located(next.trim_start(), next, block_index.strict_add(1)));
                                *block_index = block_index.strict_add(1);
                            }
                            Ok(Kind::Paragraph(body))
                        };
                        result
                    };
                    match stage_result {
                        Ok(value) => value,
                        Err(failure) => break 'parse Err(failure),
                    }
                };
                if let Some(annotation) = &pending {
                    let compatible = matches!(
                        (&*annotation.name, &kind),
                        ("ol", Kind::List(_)) | ("table", Kind::Table(_)) | ("paragraph", Kind::Paragraph(_)) | ("quote", Kind::Quote(_))
                    );
                    if !compatible {
                        break 'parse Err(error(annotation.line, format!("{} annotation has an incompatible target block", annotation.name)));
                    }
                }
                document.blocks.push(Block {
                    annotation: pending.take(),
                    kind,
                    line: start.strict_add(1),
                });
            }
            if let Some(annotation) = pending {
                break 'parse Err(error(annotation.line, "annotation has no target block"));
            }
            Ok(document)
        };
        result
    }
    .map_err(|diagnostic| vec![diagnostic])?;
    {
        // Format document.

        let document = &mut parsed_document;
        let result: Result<String, Diagnostic> = 'format_document: {
            let targets = {
                let stage_result = {
                    // Prepare references.

                    let result: Result<Targets, Diagnostic> = 'prepare_references: {
                        let mut targets = Targets::new();
                        for block in &mut document.blocks {
                            if let Some(annotation) = &block.annotation
                                && let Some(id) = annotation.options.get("id")
                            {
                                match register_target(&mut targets, id, None, annotation) {
                                    Ok(value) => value,
                                    Err(failure) => break 'prepare_references Err(failure),
                                }
                            }
                            if let Kind::List(items) = &mut block.kind {
                                {
                                    // Assign.
                                    use crate::list::{GREEK, alphabetic};

                                    let annotation: Option<&Annotation> = block.annotation.as_ref();
                                    let cycle = annotation.is_some_and(|value| value.name == "ol");
                                    let root = items.first().map_or(0, |item| item.indent);
                                    let mut numbers = BTreeMap::new();
                                    let mut ancestors: Vec<String> = Vec::new();
                                    for item in &mut *items {
                                        numbers.retain(|indent, _| *indent <= item.indent);
                                        let at_root = item.indent == root;
                                        let ordered = cycle || item.ordered;
                                        if ordered {
                                            let start = if at_root {
                                                annotation
                                                    .and_then(|value| value.options.get("start"))
                                                    .and_then(|value| value.parse().ok())
                                                    .unwrap_or(item.start)
                                            } else if cycle {
                                                1
                                            } else {
                                                item.start
                                            };
                                            let number =
                                                numbers.entry(item.indent).or_insert(start);
                                            let depth = item.depth;
                                            ancestors.truncate(depth);
                                            let label = if cycle {
                                                {
                                                    // Choose the marker for this nesting depth.

                                                    let ordinal = *number;

                                                    match depth & 3 {
                                                        0 => ordinal.to_string(),
                                                        1 => alphabetic(
                                                            ordinal,
                                                            "abcdefghijklmnopqrstuvwxyz",
                                                        ),
                                                        2 => {
                                                            // Build the Roman numeral.

                                                            let mut remainder = ordinal;

                                                            let mut output = String::new();
                                                            for (value, digits) in [
                                                                (1000, "m"),
                                                                (900, "cm"),
                                                                (500, "d"),
                                                                (400, "cd"),
                                                                (100, "c"),
                                                                (90, "xc"),
                                                                (50, "l"),
                                                                (40, "xl"),
                                                                (10, "x"),
                                                                (9, "ix"),
                                                                (5, "v"),
                                                                (4, "iv"),
                                                                (1, "i"),
                                                            ] {
                                                                while remainder >= value {
                                                                    output.push_str(digits);
                                                                    remainder =
                                                                        remainder.strict_sub(value);
                                                                }
                                                            }
                                                            output
                                                        }
                                                        _ => alphabetic(ordinal, GREEK),
                                                    }
                                                }
                                            } else {
                                                number.to_string()
                                            };
                                            item.marker = format!("{label}.");
                                            ancestors.push(label);
                                            item.path = ancestors.join(".");
                                            *number = number.saturating_add(1);
                                        } else {
                                            numbers.remove(&item.indent);
                                            item.marker = "-".into();
                                        }
                                    }
                                }
                                for item in items {
                                    if let Some(anchor) = &item.anchor
                                        && let Some(id) = anchor.options.get("id")
                                    {
                                        match register_target(
                                            &mut targets,
                                            id,
                                            Some(item.path.clone()),
                                            anchor,
                                        ) {
                                            Ok(value) => value,
                                            Err(failure) => break 'prepare_references Err(failure),
                                        }
                                    }
                                }
                            }
                        }
                        for block in &mut document.blocks {
                            match &mut block.kind {
                                Kind::Paragraph(lines) | Kind::Quote(lines) => {
                                    match text::resolve_lines(lines, &targets) {
                                        Ok(value) => value,
                                        Err(failure) => break 'prepare_references Err(failure),
                                    }
                                }
                                Kind::Heading(_, value) => match value.resolve(&targets) {
                                    Ok(value) => value,
                                    Err(failure) => break 'prepare_references Err(failure),
                                },
                                Kind::List(items) => {
                                    for item in items {
                                        match text::resolve_lines(&mut item.lines, &targets) {
                                            Ok(value) => value,
                                            Err(failure) => break 'prepare_references Err(failure),
                                        }
                                    }
                                }
                                Kind::Table(table) => match table.resolve_references(&targets) {
                                    Ok(value) => value,
                                    Err(failure) => break 'prepare_references Err(failure),
                                },
                                Kind::Literal(_) => {}
                            }
                        }
                        Ok(targets)
                    };
                    result
                };
                match stage_result {
                    Ok(value) => value,
                    Err(failure) => break 'format_document Err(failure),
                }
            };
            let reserved_ids = targets.keys().cloned().collect();
            let base_width = document
                .annotation
                .as_ref()
                .map_or(Some(80), |annotation| annotation.width(Some(80)));
            let default_style = document
                .annotation
                .as_ref()
                .and_then(|annotation| annotation.options.get("table"))
                .map_or(Style::Pipes, |style| {
                    Style::parse(style).unwrap_or(Style::Pipes)
                });
            let TableLayout {
                ids,
                widest,
                mut text,
            } = {
                let stage_result = {
                    // Measure tables.

                    let ids: BTreeSet<String> = reserved_ids;
                    let result: Result<TableLayout, Diagnostic> = 'measure_tables: {
                        let mut widest: Option<(usize, usize)> = None;
                        let mut table_text = BTreeMap::new();

                        // Tables are independent of prose width, so this pass cannot form a layout cycle.
                        for (index, block) in document.blocks.iter_mut().enumerate() {
                            if let Kind::Table(table) = &block.kind {
                                let style = block
                                    .annotation
                                    .as_ref()
                                    .and_then(|annotation| annotation.options.get("style"))
                                    .map_or(default_style, |style| {
                                        Style::parse(style).unwrap_or(default_style)
                                    });
                                if style != Style::Pipes
                                    && table.has_alignment()
                                    && block.annotation.as_ref().is_none_or(|annotation| {
                                        !annotation.options.contains_key("align")
                                    })
                                {
                                    let annotation = block.annotation.get_or_insert_with(|| {
                                        Annotation::new("table", block.line)
                                    });
                                    annotation
                                        .options
                                        .insert("align".into(), table.alignment_option());
                                }
                                let alignment = block
                                    .annotation
                                    .as_ref()
                                    .and_then(|annotation| annotation.options.get("align"));
                                let alignments =
                                    alignment.and_then(|value| Alignment::parse_list(value));
                                if let Some(alignments) = &alignments
                                    && alignments.len() != table.columns()
                                {
                                    break 'measure_tables Err(error(
                                        block.line,
                                        "align must contain one alignment per table column",
                                    ));
                                }
                                let text = table.emit(style, alignments.as_deref());
                                let width =
                                    text.lines().map(UnicodeWidthStr::width).max().unwrap_or(0);
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
                    };
                    result
                };
                match stage_result {
                    Ok(value) => value,
                    Err(failure) => break 'format_document Err(failure),
                }
            };

            let wrapping_table =
                widest.filter(|(_, width)| base_width.is_some_and(|minimum| *width > minimum));
            let effective_width = wrapping_table.map(|(_, width)| width).or(base_width);
            if let Some(document_annotation) = &mut document.annotation {
                if let Some((index, _)) = wrapping_table {
                    let Some(block) = document.blocks.get_mut(index) else {
                        break 'format_document Err(error(1, "wrapping table is missing"));
                    };
                    let annotation = block
                        .annotation
                        .get_or_insert_with(|| Annotation::new("table", block.line));
                    let id = if let Some(id) = annotation.options.get("id") {
                        id.clone()
                    } else {
                        let mut suffix: usize = 1;
                        while ids.contains(&format!("table-{suffix}")) {
                            suffix = suffix.strict_add(1);
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
                    .map_or(effective_width, |annotation| {
                        annotation.width(effective_width)
                    });
                let body = match &block.kind {
                    Kind::Paragraph(lines) => wrap_lines(lines, width, "", "", false),
                    Kind::Heading(level, heading_text) => {
                        format!("{} {}", "#".repeat(*level), heading_text.value.trim())
                    }
                    Kind::Quote(lines) => {
                        // Emit quote.

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
                    Kind::List(items) => {
                        // Emit list.

                        let annotation: Option<&Annotation> = block.annotation.as_ref();

                        let root = items.first().map_or(0, |item| item.indent);
                        let cycle = annotation.is_some_and(|value| value.name == "ol");
                        let checklist = cycle && items.iter().any(|item| item.checkbox.is_some());
                        let mut output = Vec::new();
                        for item in items {
                            let indent = " ".repeat(item.indent.strict_sub(root));
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
                    Kind::Table(_) => text.remove(&index).unwrap_or_default(),
                    Kind::Literal(literal) => literal.clone(),
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
        };
        result
    }
    .map_err(|diagnostic| vec![diagnostic])
}

/// Reserve an explicit ID and reject duplicate table or item targets.
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

/// Width=off preserves physical line boundaries; wrapping never splits tokens or
/// balanced inline backtick spans (including links with spaces inside them).
#[expect(
    clippy::too_many_lines,
    reason = "Inline tokenization keeps wrapping and its block-boundary guards together"
)]
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
    let tokens = {
        // Tokens.
        use crate::inline::protected_len;
        let prose = text.as_str();

        let mut tokens = Vec::new();
        let mut begin: usize = 0;
        let mut offset: usize = 0;
        while offset < prose.len() {
            if let Some(length) = protected_len(prose.split_at(offset).1) {
                offset = offset.strict_add(length);
                continue;
            }
            let ch = prose.split_at(offset).1.chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                if begin < offset {
                    tokens.push(prose.split_at(offset).0.split_at(begin).1);
                }
                begin = offset.strict_add(ch.len_utf8());
            }
            offset = offset.strict_add(ch.len_utf8());
        }
        if begin < prose.len() {
            tokens.push(prose.split_at(begin).1);
        }
        tokens
    };
    let mut output = Vec::new();
    let mut current = first.to_owned();
    let mut has_word = false;
    for token in tokens {
        let needed = UnicodeWidthStr::width(current.as_str())
            .strict_add(usize::from(has_word))
            .strict_add(UnicodeWidthStr::width(token));
        if has_word
            && needed > limit
            && !{
                // Unsafe line start.

                parser::starts_block(token) || (cycle && parser::cycle_marker(token).is_some())
            }
            && current.trim() != "---"
            && !{
                // Ascii rule.

                let current_text: &str = current.trim();
                inline::ascii_rule(current_text).is_ok()
            }
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

#[expect(
    clippy::integer_division_remainder_used,
    reason = "Backslash parity uses public document text and needs no constant-time arithmetic"
)]
/// Split a line from a trailing two-space or odd-backslash hard-break marker.
fn hard_break(line: &str) -> Option<(&str, &str)> {
    if line.ends_with("  ") {
        return Some((line.trim_end(), line.split_at(line.trim_end().len()).1));
    }
    let slashes = line
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count();
    (slashes % 2 == 1).then(|| (line.strip_suffix('\\').unwrap_or(line), "\\"))
}
