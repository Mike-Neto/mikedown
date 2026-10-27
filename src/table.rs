use nom::branch::alt;
use nom::bytes::complete::{take_while, take_while_m_n};
use nom::character::complete::one_of;
use nom::combinator::{all_consuming, recognize};
use nom::sequence::delimited;
use nom::{IResult, Parser as _};
use unicode_width::UnicodeWidthStr as _;

use crate::text::{Targets, Text};
use crate::{Diagnostic, inline};

/// Validated rectangular rows.
pub struct Table {
    /// Header followed by body rows, all with equal cell counts.
    pub rows: Vec<Vec<Text>>,
}

impl Table {
    /// Escape cells, measure columns, and render left-aligned Markdown pipes.
    pub(crate) fn emit(&self) -> String {
        let rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        let mut output = String::new();
                        for fragment in inline::fragments(&cell.value, true) {
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
        let mut widths = vec![3; self.rows.first().map_or(0, Vec::len)];
        for row in &rows {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.width());
            }
        }
        let mut output = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let cells = row
                .iter()
                .zip(&widths)
                .map(|(cell, width)| {
                    // Column measurement includes every cell; padding cannot be negative.
                    format!(" {cell}{} ", " ".repeat(width.strict_sub(cell.width())))
                })
                .collect::<Vec<_>>();
            output.push(format!("|{}|", cells.join("|")));
            if row_index == 0 {
                let rules = widths
                    .iter()
                    .map(|width| format!(" {} ", "-".repeat(*width)))
                    .collect::<Vec<_>>();
                output.push(format!("|{}|", rules.join("|")));
            }
        }
        output.join("\n")
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

/// Recognize a Markdown header separator, accepting legacy alignment colons.
pub fn separator_cell(input: &str) -> IResult<&str, &str> {
    // Preserve the existing acceptance of repeated leading/trailing colons.
    all_consuming(recognize(delimited(
        take_while(|ch| ch == ':'),
        take_while_m_n(3, usize::MAX, |ch| ch == '-'),
        take_while(|ch| ch == ':'),
    )))
    .parse(input)
}
