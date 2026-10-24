//! Shared inline recognition. Prose treats backslashes literally; tables protect
//! the following scalar. Both retain exact spelling and match backtick-run lengths.

use core::iter;

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
    iter::from_fn(move || {
        let (rest, text) = piece(remaining, escapes).ok()?;
        remaining = rest;
        let ticks = text.starts_with('`');
        let protected = code != 0 || ticks || (escapes && text.starts_with('\\'));
        if ticks {
            code = match code {
                0 => text.len(),
                length if length == text.len() => 0,
                length => length,
            };
        }
        Some(Fragment { text, protected })
    })
}

fn balanced(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0;
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
            depth += 1;
        }
        if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(offset + ch.len_utf8());
            }
        }
    }
    None
}

fn protected_len(text: &str) -> Option<usize> {
    if text.starts_with('`') {
        let count = text.bytes().take_while(|b| *b == b'`').count();
        let mut offset = count;
        while offset < text.len() {
            if text[offset..].starts_with('`') {
                let run = text[offset..].bytes().take_while(|b| *b == b'`').count();
                if run == count {
                    return Some(offset + run);
                }
                offset += run;
            } else {
                offset += text[offset..].chars().next()?.len_utf8();
            }
        }
    }
    let image = usize::from(text.starts_with("!["));
    if text[image..].starts_with('[') {
        let end = image + balanced(&text[image..], '[', ']')?;
        if text[end..].starts_with('(') {
            return Some(end + balanced(&text[end..], '(', ')')?);
        }
        if text[end..].starts_with('[') {
            return Some(end + balanced(&text[end..], '[', ']')?);
        }
        return Some(end);
    }
    if text.starts_with('<') {
        let end = text.find('>')?;
        let body = &text[1..end];
        if !body.chars().any(char::is_whitespace) && (body.contains(':') || body.contains('@')) {
            return Some(end + 1);
        }
    }
    None
}

pub(crate) fn tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut begin = 0;
    let mut offset = 0;
    while offset < text.len() {
        if let Some(length) = protected_len(&text[offset..]) {
            offset += length;
            continue;
        }
        let ch = text[offset..].chars().next().unwrap_or(' ');
        if ch.is_whitespace() {
            if begin < offset {
                tokens.push(&text[begin..offset]);
            }
            begin = offset + ch.len_utf8();
        }
        offset += ch.len_utf8();
    }
    if begin < text.len() {
        tokens.push(&text[begin..]);
    }
    tokens
}

pub(crate) struct ItemReference<'source> {
    pub start: usize,
    pub end: usize,
    pub target_offset: usize,
    pub id: &'source str,
}

pub(crate) fn item_references(text: &str) -> Vec<ItemReference<'_>> {
    let mut references = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let tail = &text[offset..];
        if let Some(escaped) = tail.strip_prefix('\\') {
            offset += 1 + escaped.chars().next().map_or(0, char::len_utf8);
            continue;
        }
        if tail.starts_with('[')
            && let Some(label_end) = balanced(tail, '[', ']')
            && tail[label_end..].starts_with('(')
            && let Some(destination_end) = balanced(&tail[label_end..], '(', ')')
        {
            let end = label_end + destination_end;
            let destination = &tail[label_end + 1..end - 1];
            if let Some(id) = destination.strip_prefix("item:") {
                references.push(ItemReference {
                    start: offset,
                    end: offset + end,
                    target_offset: offset + label_end + 1 + "item:".len(),
                    id,
                });
            }
            offset += end;
            continue;
        }
        if let Some(length) = protected_len(tail) {
            offset += length;
        } else {
            offset += tail.chars().next().map_or(1, char::len_utf8);
        }
    }
    references
}

pub(crate) fn cells(text: &str, delimiter: char) -> Vec<&str> {
    let mut cells = Vec::new();
    let mut begin = 0;
    let mut offset = 0;
    for fragment in fragments(text, true) {
        if !fragment.protected && fragment.text.starts_with(delimiter) {
            cells.push(text[begin..offset].trim());
            begin = offset + fragment.text.len();
        }
        offset += fragment.text.len();
    }
    cells.push(text[begin..].trim());
    cells
}

pub(crate) fn escape_delimiters(text: &str) -> String {
    let mut output = String::new();
    for fragment in fragments(text, true) {
        if matches!(fragment.text, "|" | "│") {
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
