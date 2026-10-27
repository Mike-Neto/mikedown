use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while_m_n};
use nom::character::complete::{char, one_of};
use nom::combinator::{all_consuming, recognize, value as nom_value};
use nom::multi::separated_list1;
use nom::sequence::delimited;
use nom::{IResult, Parser as _};
use unicode_width::UnicodeWidthStr as _;

use crate::text::{Targets, Text};
use crate::{Diagnostic, inline};

/// Available table border styles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Style {
    /// ASCII grid borders.
    Ascii,
    /// Markdown pipe rows and header separator.
    Pipes,
    /// Unicode box-drawing borders.
    Unicode,
}

impl Style {
    /// Parse a canonical table style name.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        {
            // Style.

            let input: &str = value;
            let result: IResult<&str, Self> = {
                all_consuming(alt((
                    nom_value(Self::Unicode, tag("unicode")),
                    nom_value(Self::Ascii, tag("ascii")),
                    nom_value(Self::Pipes, tag("pipes")),
                )))
                .parse(input)
            };
            result
        }
        .ok()
        .map(|(_, style)| style)
    }
}

/// Horizontal cell alignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alignment {
    /// Center cells within their column.
    Center,
    /// Align cells to the left edge.
    Left,
    /// Align cells to the right edge.
    Right,
}

impl Alignment {
    /// Parse a comma-separated list of alignment names.
    pub(crate) fn parse_list(value: &str) -> Option<Vec<Self>> {
        {
            // Alignment list.

            let input: &str = value;
            let result: IResult<&str, Vec<Self>> = {
                all_consuming(separated_list1(
                    char(','),
                    alt((
                        nom_value(Self::Left, tag("left")),
                        nom_value(Self::Right, tag("right")),
                        nom_value(Self::Center, tag("center")),
                    )),
                ))
                .parse(input)
            };
            result
        }
        .ok()
        .map(|(_, alignments)| alignments)
    }
}

/// Validated rectangular rows with one alignment per column.
pub struct Table {
    /// Column alignments in left-to-right order.
    pub alignments: Vec<Alignment>,
    /// Header followed by body rows, all with equal cell counts.
    pub rows: Vec<Vec<Text>>,
}

impl Table {
    /// Serialize column alignments as an annotation option value.
    pub(crate) fn alignment_option(&self) -> String {
        self.alignments
            .iter()
            .map(|alignment| match alignment {
                Alignment::Left => "left",
                Alignment::Right => "right",
                Alignment::Center => "center",
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Return the number of cells in the header.
    pub(crate) fn columns(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "Table emission keeps cell escaping inline"
    )]
    /// Escape cells, measure columns, and render the requested border style.
    pub(crate) fn emit(&self, style: Style, override_alignments: Option<&[Alignment]>) -> String {
        let rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        // Escape delimiters.

                        let text: &str = &cell.value;

                        let mut output = String::new();
                        for fragment in inline::fragments(text, true) {
                            if matches!(fragment.text, "|" | "│") {
                                output.push('\\');
                            }
                            output.push_str(fragment.text);
                        }
                        output
                    })
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
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.width());
            }
        }
        let border = |left: char, middle: char, right: char, rule: char| {
            let cells = widths
                .iter()
                .map(|width| rule.to_string().repeat(width.strict_add(2)))
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
                .zip(&widths)
                .zip(alignments)
                .map(|((cell, width), alignment)| {
                    // Column measurement includes every cell; padding cannot be negative.
                    let padding = width.strict_sub(cell.width());
                    let left = match alignment {
                        Alignment::Left => 0,
                        Alignment::Right => padding,
                        #[expect(
                            clippy::integer_division,
                            clippy::integer_division_remainder_used,
                            reason = "Centering intentionally rounds down public padding and needs no constant-time arithmetic"
                        )]
                        Alignment::Center => padding / 2,
                    };
                    format!(" {}{cell}{} ", " ".repeat(left), " ".repeat(padding.strict_sub(left)))
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
                            .zip(alignments)
                            .map(|(width, alignment)| {
                                // Minimum widths reserve space for alignment colons.
                                let rule = match alignment {
                                    Alignment::Left => "-".repeat(*width),
                                    Alignment::Right => {
                                        format!("{}:", "-".repeat(width.strict_sub(1)))
                                    }
                                    Alignment::Center => {
                                        format!(":{}:", "-".repeat(width.strict_sub(2)))
                                    }
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

    /// Test whether any column has nondefault alignment.
    pub(crate) fn has_alignment(&self) -> bool {
        self.alignments
            .iter()
            .any(|alignment| *alignment != Alignment::Left)
    }

    /// Resolve item links in all cells before measuring their display widths.
    pub(crate) fn resolve_references(&mut self, targets: &Targets) -> Result<(), Diagnostic> {
        for row in &mut self.rows {
            for cell in row {
                cell.resolve(targets)?;
            }
        }
        Ok(())
    }

    /// Test whether a trimmed line has a table row or border prefix.
    pub(crate) fn starts(line: &str) -> bool {
        {
            // Table start.

            let input: &str = line.trim();
            let result: IResult<&str, &str> =
                { alt((recognize(one_of("|│┌├└")), inline::ascii_rule)).parse(input) };
            result
        }
        .is_ok()
    }
}

/// Recognize a Markdown alignment separator, preserving repeated colons.
pub fn separator_cell(input: &str) -> IResult<&str, &str> {
    // Preserve the existing acceptance of repeated leading/trailing colons.
    all_consuming(recognize(delimited(
        take_while(|ch| ch == ':'),
        take_while_m_n(3, usize::MAX, |ch| ch == '-'),
        take_while(|ch| ch == ':'),
    )))
    .parse(input)
}
