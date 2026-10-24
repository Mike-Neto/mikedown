//! Shared inline recognition. Prose treats backslashes literally; tables protect
//! the following scalar. Both retain exact spelling and match backtick-run lengths.

use nom::branch::alt;
use nom::bytes::complete::{take_while, take_while1};
use nom::character::complete::{anychar, char};
use nom::combinator::{opt, recognize};
use nom::{IResult, Parser};

pub(crate) struct Fragment<'source> {
    pub text: &'source str,
    pub protected: bool,
}

fn piece(input: &str, escapes: bool) -> IResult<&str, &str> {
    if escapes && input.starts_with('\\') {
        return recognize((char('\\'), opt(anychar))).parse(input);
    }
    alt((
        take_while1(|ch| ch == '`'),
        take_while1(char::is_whitespace),
        take_while1(|ch: char| {
            !ch.is_whitespace() && !matches!(ch, '`' | '|' | '│') && (!escapes || ch != '\\')
        }),
        recognize(anychar),
    ))
    .parse(input)
}

pub(crate) fn fragments(text: &str, escapes: bool) -> impl Iterator<Item = Fragment<'_>> {
    let mut remaining = text;
    let mut code = 0;
    std::iter::from_fn(move || {
        let (rest, text) = piece(remaining, escapes).ok()?;
        remaining = rest;
        let ticks = text.starts_with('`');
        let protected = code != 0 || ticks || (escapes && text.starts_with('\\'));
        if ticks {
            if code == 0 {
                code = text.len();
            } else if code == text.len() {
                code = 0;
            }
        }
        Some(Fragment { text, protected })
    })
}

pub(crate) fn tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut begin = 0;
    let mut offset = 0;
    for fragment in fragments(text, false) {
        if !fragment.protected && fragment.text.chars().all(char::is_whitespace) {
            if begin < offset {
                tokens.push(&text[begin..offset]);
            }
            begin = offset + fragment.text.len();
        }
        offset += fragment.text.len();
    }
    if begin < text.len() {
        tokens.push(&text[begin..]);
    }
    tokens
}

pub(crate) fn cells(text: &str, delimiter: char) -> Vec<String> {
    let mut cells = Vec::new();
    let mut begin = 0;
    let mut offset = 0;
    for fragment in fragments(text, true) {
        if !fragment.protected && fragment.text.starts_with(delimiter) {
            cells.push(text[begin..offset].trim().to_owned());
            begin = offset + fragment.text.len();
        }
        offset += fragment.text.len();
    }
    cells.push(text[begin..].trim().to_owned());
    cells
}

pub(crate) fn escape_delimiters(text: &str) -> String {
    let mut output = String::new();
    for fragment in fragments(text, true) {
        if !fragment.protected && matches!(fragment.text, "|" | "│") {
            output.push('\\');
        }
        output.push_str(fragment.text);
    }
    output
}

// Table borders also need recognition in the wrapping guard.
pub(crate) fn ascii_rule(input: &str) -> IResult<&str, &str> {
    use nom::combinator::{all_consuming, verify};

    all_consuming(recognize((
        char('+'),
        verify(take_while(|ch| ch == '+' || ch == '-'), |body: &str| {
            body.contains('-')
        }),
    )))
    .parse(input)
}
