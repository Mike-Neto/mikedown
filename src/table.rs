use unicode_width::UnicodeWidthStr;

use crate::{Diagnostic, error};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Style {
    Unicode,
    Ascii,
    Pipes,
}

impl Style {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "unicode" => Some(Self::Unicode),
            "ascii" => Some(Self::Ascii),
            "pipes" => Some(Self::Pipes),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Alignment {
    Left,
    Right,
    Center,
}

impl Alignment {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "right" => Self::Right,
            "center" => Self::Center,
            _ => Self::Left,
        }
    }
}

pub(crate) struct Table {
    rows: Vec<Vec<String>>,
    alignments: Vec<Alignment>,
}

impl Table {
    pub(crate) fn starts(line: &str) -> bool {
        let text = line.trim();
        text.starts_with('|')
            || text.starts_with('│')
            || text.starts_with('┌')
            || text.starts_with('├')
            || text.starts_with('└')
            || ascii_border(text)
    }

    pub(crate) fn columns(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }

    pub(crate) fn has_alignment(&self) -> bool {
        self.alignments.iter().any(|a| *a != Alignment::Left)
    }

    pub(crate) fn alignment_option(&self) -> String {
        self.alignments
            .iter()
            .map(|a| match a {
                Alignment::Left => "left",
                Alignment::Right => "right",
                Alignment::Center => "center",
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    pub(crate) fn parse(lines: &[&str], start: usize) -> Result<Self, Diagnostic> {
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut alignments = Vec::new();
        let mut header_boundary = false;
        for (offset, line) in lines.iter().enumerate() {
            let text = line.trim();
            if ascii_border(text) || text.starts_with(['┌', '├', '└']) {
                validate_border(text, start + offset)?;
                continue;
            }
            let delimiter = if text.starts_with('│') { '│' } else { '|' };
            if !text.ends_with(delimiter) || text.len() <= delimiter.len_utf8() {
                return Err(error(
                    start + offset,
                    "table rows must have opening and closing delimiters",
                ));
            }
            let body = &text[delimiter.len_utf8()..text.len() - delimiter.len_utf8()];
            let cells = split_cells(body, delimiter);
            let separator = delimiter == '|'
                && cells.iter().all(|cell| {
                    let rule = cell.trim_matches(':');
                    rule.len() >= 3 && rule.bytes().all(|ch| ch == b'-')
                });
            if let Some(previous) = rows.first()
                && cells.len() != previous.len()
            {
                return Err(error(start + offset, "inconsistent table column count"));
            }
            if separator {
                if rows.len() != 1 || header_boundary {
                    return Err(error(
                        start + offset,
                        "table separator must follow the header",
                    ));
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
                rows.push(
                    cells
                        .into_iter()
                        .map(|cell| escape_delimiters(&cell))
                        .collect(),
                );
            }
        }
        if rows.is_empty() {
            return Err(error(start, "table has no header row"));
        }
        if alignments.is_empty() {
            alignments = vec![Alignment::Left; rows[0].len()];
        }
        Ok(Self { rows, alignments })
    }

    pub(crate) fn emit(&self, style: Style, override_alignments: Option<&[Alignment]>) -> String {
        let alignments = override_alignments.unwrap_or(&self.alignments);
        let mut widths = alignments
            .iter()
            .map(|alignment| {
                if style == Style::Pipes {
                    match alignment {
                        Alignment::Left => 3,
                        Alignment::Right => 4,
                        Alignment::Center => 5,
                    }
                } else {
                    1
                }
            })
            .collect::<Vec<_>>();
        for row in &self.rows {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(cell.width());
            }
        }
        let border = |left: char, middle: char, right: char, rule: char| {
            let cells = widths
                .iter()
                .map(|width| rule.to_string().repeat(width + 2))
                .collect::<Vec<_>>();
            format!("{left}{}{right}", cells.join(&middle.to_string()))
        };
        let mut output = Vec::new();
        match style {
            Style::Unicode => output.push(border('┌', '┬', '┐', '─')),
            Style::Ascii => output.push(border('+', '+', '+', '-')),
            Style::Pipes => {}
        }
        let delimiter = if style == Style::Unicode { '│' } else { '|' };
        for (row_index, row) in self.rows.iter().enumerate() {
            let cells = row
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let padding = widths[index] - cell.width();
                    let left = match alignments[index] {
                        Alignment::Left => 0,
                        Alignment::Right => padding,
                        Alignment::Center => padding / 2,
                    };
                    format!(" {}{cell}{} ", " ".repeat(left), " ".repeat(padding - left))
                })
                .collect::<Vec<_>>();
            output.push(format!(
                "{delimiter}{}{delimiter}",
                cells.join(&delimiter.to_string())
            ));
            if row_index == 0 {
                match style {
                    Style::Unicode if self.rows.len() > 1 => {
                        output.push(border('├', '┼', '┤', '─'));
                    }
                    Style::Ascii if self.rows.len() > 1 => output.push(border('+', '+', '+', '-')),
                    Style::Unicode | Style::Ascii => {}
                    Style::Pipes => {
                        let rules = widths
                            .iter()
                            .enumerate()
                            .map(|(index, width)| {
                                let rule = match alignments[index] {
                                    Alignment::Left => "-".repeat(*width),
                                    Alignment::Right => format!("{}:", "-".repeat(width - 1)),
                                    Alignment::Center => format!(":{}:", "-".repeat(width - 2)),
                                };
                                format!(" {rule} ")
                            })
                            .collect::<Vec<_>>();
                        output.push(format!("|{}|", rules.join("|")));
                    }
                }
            }
        }
        match style {
            Style::Unicode => output.push(border('└', '┴', '┘', '─')),
            Style::Ascii => output.push(border('+', '+', '+', '-')),
            Style::Pipes => {}
        }
        output.join("\n")
    }
}

fn ascii_border(text: &str) -> bool {
    text.starts_with('+') && text.chars().all(|ch| ch == '+' || ch == '-') && text.contains('-')
}

fn validate_border(text: &str, line: usize) -> Result<(), Diagnostic> {
    let valid = if text.starts_with('+') {
        text.ends_with('+') && text.chars().all(|ch| ch == '+' || ch == '-')
    } else {
        let (right, middle) = match text.chars().next() {
            Some('┌') => ('┐', '┬'),
            Some('├') => ('┤', '┼'),
            _ => ('┘', '┴'),
        };
        text.ends_with(right)
            && text
                .chars()
                .skip(1)
                .take(text.chars().count().saturating_sub(2))
                .all(|ch| ch == '─' || ch == middle)
    };
    if valid {
        Ok(())
    } else {
        Err(error(line, "malformed table border"))
    }
}

// Preserve the original spelling of escapes and inline code. A backslash escapes
// one scalar, and only matching backtick-run lengths terminate inline code.
fn split_cells(text: &str, delimiter: char) -> Vec<String> {
    let mut cells = Vec::new();
    let mut begin = 0;
    let mut escaped = false;
    let mut code = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '`' {
            let mut count = 1;
            while chars.peek().is_some_and(|(_, next)| *next == '`') {
                chars.next();
                count += 1;
            }
            if code == 0 {
                code = count;
            } else if code == count {
                code = 0;
            }
        } else if ch == delimiter && code == 0 {
            cells.push(text[begin..index].trim().to_owned());
            begin = index + ch.len_utf8();
        }
    }
    cells.push(text[begin..].trim().to_owned());
    cells
}

fn escape_delimiters(text: &str) -> String {
    let mut output = String::new();
    let mut escaped = false;
    let mut code = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            output.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            output.push(ch);
            escaped = true;
            continue;
        }
        if ch == '`' {
            let mut count = 1;
            while chars.peek() == Some(&'`') {
                chars.next();
                count += 1;
            }
            output.push_str(&"`".repeat(count));
            if code == 0 {
                code = count;
            } else if code == count {
                code = 0;
            }
        } else {
            if code == 0 && (ch == '|' || ch == '│') {
                output.push('\\');
            }
            output.push(ch);
        }
    }
    output
}
