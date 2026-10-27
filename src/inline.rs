//! Shared inline recognition. Prose treats backslashes literally; tables protect
//! the following scalar. Both retain exact spelling and match backtick-run lengths.

use core::iter;

use nom::branch::alt;
use nom::bytes::complete::{take_while, take_while1};
use nom::character::complete::{anychar, char};
use nom::combinator::{opt, recognize};
use nom::{IResult, Parser as _};

/// An inline span retaining its original spelling.
pub struct Fragment<'source> {
    /// Whether delimiters or whitespace in this span must remain literal.
    pub protected: bool,
    /// Borrowed source spelling of the span.
    pub text: &'source str,
}

/// A link to a list item, expressed as byte offsets into the source text.
pub struct ItemReference<'source> {
    /// Byte offset immediately after the complete link.
    pub end: usize,
    /// Unmodified item ID from the link destination.
    pub id: &'source str,
    /// Byte offset at the opening bracket.
    pub start: usize,
    /// Byte offset at the first character of the target ID.
    pub target_offset: usize,
}

/// Split inline text into spans, optionally protecting backslash escapes.
pub fn fragments(text: &str, escapes: bool) -> impl Iterator<Item = Fragment<'_>> {
    let mut remaining = text;
    let mut code = 0;
    iter::from_fn(move || {
        let (rest, piece) = {
            // Recognize one protected or ordinary inline fragment.

            let input: &str = remaining;
            let result: IResult<&str, &str> = 'piece: {
                if escapes && input.starts_with('\\') {
                    break 'piece recognize((char('\\'), opt(anychar))).parse(input);
                }
                alt((
                    take_while1(|ch| ch == '`'),
                    take_while1(char::is_whitespace),
                    take_while1(|ch: char| {
                        !ch.is_whitespace()
                            && !matches!(ch, '`' | '|' | '│')
                            && (!escapes || ch != '\\')
                    }),
                    recognize(anychar),
                ))
                .parse(input)
            };
            result
        }
        .ok()?;
        remaining = rest;
        let ticks = piece.starts_with('`');
        let protected = code != 0 || ticks || (escapes && piece.starts_with('\\'));
        if ticks {
            code = match code {
                0 => piece.len(),
                length if length == piece.len() => 0,
                length => length,
            };
        }
        Some(Fragment {
            protected,
            text: piece,
        })
    })
}

/// Find the byte offset after a balanced delimiter pair, ignoring escaped scalars.
pub fn balanced(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth: usize = 0;
    let mut escaped = false;
    for (offset, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == open {
            depth = depth.checked_add(1)?;
        }
        if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return offset.checked_add(ch.len_utf8());
            }
        }
    }
    None
}

/// Return the byte length of a protected code span, link, image, or autolink.
pub fn protected_len(text: &str) -> Option<usize> {
    if text.starts_with('`') {
        let count = text.bytes().take_while(|byte| *byte == b'`').count();
        let mut offset = count;
        while offset < text.len() {
            let tail = text.get(offset..)?;
            if tail.starts_with('`') {
                let run = tail.bytes().take_while(|byte| *byte == b'`').count();
                if run == count {
                    return offset.checked_add(run);
                }
                offset = offset.checked_add(run)?;
            } else {
                offset = offset.checked_add(tail.chars().next()?.len_utf8())?;
            }
        }
    }
    let image = usize::from(text.starts_with("!["));
    let label = text.get(image..)?;
    if label.starts_with('[') {
        let end = image.checked_add(balanced(label, '[', ']')?)?;
        let destination = text.get(end..)?;
        if destination.starts_with('(') {
            return end.checked_add(balanced(destination, '(', ')')?);
        }
        if destination.starts_with('[') {
            return end.checked_add(balanced(destination, '[', ']')?);
        }
        return Some(end);
    }
    if text.starts_with('<') {
        let end = text.find('>')?;
        let body = text.get(1..end)?;
        if !body.chars().any(char::is_whitespace) && (body.contains(':') || body.contains('@')) {
            return end.checked_add(1);
        }
    }
    None
}

/// Split a table row at unprotected delimiters and trim each cell.
pub fn cells(text: &str, delimiter: char) -> Vec<&str> {
    let mut cells = Vec::new();
    // Fragment lengths partition text, so offsets stay within its UTF-8 boundaries.
    let mut begin: usize = 0;
    let mut offset: usize = 0;
    for fragment in fragments(text, true) {
        if !fragment.protected && fragment.text.starts_with(delimiter) {
            cells.push(text.split_at(offset).0.split_at(begin).1.trim());
            begin = offset.strict_add(fragment.text.len());
        }
        offset = offset.strict_add(fragment.text.len());
    }
    cells.push(text.split_at(begin).1.trim());
    cells
}

// Table borders also need recognition in the wrapping guard.
/// Recognize an ASCII table border containing at least one dash.
pub fn ascii_rule(input: &str) -> IResult<&str, &str> {
    use nom::combinator::{all_consuming, verify};

    all_consuming(recognize((
        char('+'),
        verify(take_while(|ch| ch == '+' || ch == '-'), |body: &str| {
            body.contains('-')
        }),
    )))
    .parse(input)
}
