use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while_m_n};
use nom::character::complete::{char, one_of};
use nom::combinator::{all_consuming, map_res, recognize, rest, value, verify};
use nom::multi::separated_list1;
use nom::sequence::delimited;
use nom::{IResult, Parser};
use unicode_width::UnicodeWidthStr;

use crate::text::{Targets, Text};
use crate::{Diagnostic, error, inline};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Style {
    Unicode,
    Ascii,
    Pipes,
}

impl Style {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        style(value).ok().map(|(_, style)| style)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Alignment {
    Left,
    Right,
    Center,
}

impl Alignment {
    pub(crate) fn parse_list(value: &str) -> Option<Vec<Self>> {
        alignment_list(value).ok().map(|(_, alignments)| alignments)
    }
}

pub(crate) struct Table {
    rows: Vec<Vec<Text>>,
    alignments: Vec<Alignment>,
}

impl Table {
    pub(crate) fn starts(line: &str) -> bool {
        table_start(line.trim()).is_ok()
    }

    pub(crate) fn separator(line: &str) -> bool {
        let text = line.trim().trim_matches('|');
        let cells = inline::cells(text, '|');
        !cells.is_empty() && cells.iter().all(|cell| separator_cell(cell).is_ok())
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

    pub(crate) fn parse(
        lines: &[&str],
        originals: &[&str],
        start: usize,
    ) -> Result<Self, Diagnostic> {
        let mut rows: Vec<Vec<Text>> = Vec::new();
        let mut alignments = Vec::new();
        let mut header_boundary = false;
        for (offset, line) in lines.iter().enumerate() {
            let text = line.trim();
            if ascii_border(text) || text.starts_with(['┌', '├', '└']) {
                validate_border(text, start + offset)?;
                continue;
            }
            let parsed = row(text).or_else(|err| {
                if text.contains('|') && !text.starts_with(['│', '|']) {
                    Ok((
                        "",
                        ('|', text.trim_start_matches('|').trim_end_matches('|')),
                    ))
                } else {
                    Err(err)
                }
            });
            let (_, (delimiter, body)) = parsed.map_err(|_parse_error| {
                error(
                    start + offset,
                    "table rows must have opening and closing delimiters",
                )
            })?;
            let cells = inline::cells(body, delimiter);
            let separator =
                delimiter == '|' && cells.iter().all(|cell| separator_cell(cell).is_ok());
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
                let original = originals[offset];
                let mut cursor = 0;
                rows.push(
                    cells
                        .into_iter()
                        .map(|cell| {
                            let begin = cursor + original[cursor..].find(cell).unwrap_or(0);
                            cursor = begin + cell.len();
                            Text::at(cell, start + offset, original[..begin].chars().count() + 1)
                        })
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

    pub(crate) fn resolve_references(&mut self, targets: &Targets) -> Result<(), Diagnostic> {
        for row in &mut self.rows {
            for cell in row {
                cell.resolve(targets)?;
            }
        }
        Ok(())
    }

    pub(crate) fn emit(&self, style: Style, override_alignments: Option<&[Alignment]>) -> String {
        let rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| inline::escape_delimiters(&cell.value))
                    .collect()
            })
            .collect();
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
        for row in &rows {
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
        for (row_index, row) in rows.iter().enumerate() {
            let cells = row
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let padding = widths[index] - cell.width();
                    let left = match alignments[index] {
                        Alignment::Left => 0,
                        Alignment::Right => padding,
                        #[expect(
                            clippy::integer_division,
                            clippy::integer_division_remainder_used,
                            reason = "Centering intentionally rounds down public padding and needs no constant-time arithmetic"
                        )]
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

fn style(input: &str) -> IResult<&str, Style> {
    all_consuming(alt((
        value(Style::Unicode, tag("unicode")),
        value(Style::Ascii, tag("ascii")),
        value(Style::Pipes, tag("pipes")),
    )))
    .parse(input)
}

fn alignment_list(input: &str) -> IResult<&str, Vec<Alignment>> {
    all_consuming(separated_list1(
        char(','),
        alt((
            value(Alignment::Left, tag("left")),
            value(Alignment::Right, tag("right")),
            value(Alignment::Center, tag("center")),
        )),
    ))
    .parse(input)
}

fn table_start(input: &str) -> IResult<&str, &str> {
    alt((recognize(one_of("|│┌├└")), inline::ascii_rule)).parse(input)
}

fn ascii_border(text: &str) -> bool {
    inline::ascii_rule(text).is_ok()
}

fn row(input: &str) -> IResult<&str, (char, &str)> {
    let (input, delimiter) = one_of("|│").parse(input)?;
    let (input, body) =
        map_res(rest, |body: &str| body.strip_suffix(delimiter).ok_or(())).parse(input)?;
    Ok((input, (delimiter, body)))
}

fn separator_cell(input: &str) -> IResult<&str, &str> {
    // Preserve the existing acceptance of repeated leading/trailing colons.
    all_consuming(recognize(delimited(
        take_while(|ch| ch == ':'),
        take_while_m_n(3, usize::MAX, |ch| ch == '-'),
        take_while(|ch| ch == ':'),
    )))
    .parse(input)
}

fn border(input: &str) -> IResult<&str, &str> {
    all_consuming(alt((
        verify(inline::ascii_rule, |text: &str| text.ends_with('+')),
        recognize(delimited(
            char('┌'),
            take_while(|ch| ch == '─' || ch == '┬'),
            char('┐'),
        )),
        recognize(delimited(
            char('├'),
            take_while(|ch| ch == '─' || ch == '┼'),
            char('┤'),
        )),
        recognize(delimited(
            char('└'),
            take_while(|ch| ch == '─' || ch == '┴'),
            char('┘'),
        )),
    )))
    .parse(input)
}

fn validate_border(text: &str, line: usize) -> Result<(), Diagnostic> {
    border(text)
        .map(|_| ())
        .map_err(|_parse_error| error(line, "malformed table border"))
}
