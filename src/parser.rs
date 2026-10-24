use std::collections::BTreeMap;

use crate::table::{Style, Table};
use crate::{Annotation, Diagnostic, error};

pub(crate) struct Document {
    pub annotation: Option<Annotation>,
    pub blocks: Vec<Block>,
}

pub(crate) struct Block {
    pub annotation: Option<Annotation>,
    pub kind: Kind,
    pub line: usize,
}

pub(crate) enum Kind {
    Paragraph(Vec<String>),
    Heading(usize, String),
    Quote(Vec<String>),
    List(Vec<ListItem>),
    Table(Table),
    Literal(String),
}

pub(crate) struct ListItem {
    pub indent: usize,
    pub ordered: bool,
    pub lines: Vec<String>,
}

pub(crate) fn parse(source: &str) -> Result<Document, Diagnostic> {
    let lines: Vec<&str> = source.lines().collect();
    let mut document = Document {
        annotation: None,
        blocks: Vec::new(),
    };
    let mut index = 0;
    let mut pending: Option<Annotation> = None;
    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        if is_annotation(line) {
            let annotation = parse_annotation(line, index + 1)?;
            if let Some(previous) = pending.take() {
                return Err(error(previous.line, "annotation has no target block"));
            }
            if annotation.name == "document" {
                if document.annotation.is_some() || !document.blocks.is_empty() {
                    return Err(error(
                        index + 1,
                        "document annotation must occur once, before all blocks",
                    ));
                }
                document.annotation = Some(annotation);
            } else {
                pending = Some(annotation);
            }
            index += 1;
            continue;
        }
        let start = index;
        let kind = parse_block(&lines, &mut index)?;
        if let Some(annotation) = &pending {
            let compatible = matches!(
                (&*annotation.name, &kind),
                ("ol" | "ul", Kind::List(_))
                    | ("table", Kind::Table(_))
                    | ("paragraph", Kind::Paragraph(_))
                    | ("quote", Kind::Quote(_))
            );
            if !compatible {
                return Err(error(
                    annotation.line,
                    format!(
                        "{} annotation has an incompatible target block",
                        annotation.name
                    ),
                ));
            }
        }
        document.blocks.push(Block {
            annotation: pending.take(),
            kind,
            line: start + 1,
        });
    }
    if let Some(annotation) = pending {
        return Err(error(annotation.line, "annotation has no target block"));
    }
    Ok(document)
}

fn parse_block(lines: &[&str], index: &mut usize) -> Result<Kind, Diagnostic> {
    let start = *index;
    let line = lines[start];
    if let Some((character, count)) = fence(line) {
        return parse_fence(lines, index, character, count);
    }
    if let Some((level, text)) = heading(line) {
        *index += 1;
        return Ok(Kind::Heading(level, text.to_owned()));
    }
    if line.trim() == "---" {
        *index += 1;
        return Ok(Kind::Literal("---".into()));
    }
    if is_quote(line) {
        let mut body = Vec::new();
        while *index < lines.len() && is_quote(lines[*index]) {
            let content = lines[*index]
                .trim_start()
                .strip_prefix('>')
                .unwrap_or_default();
            body.push(
                content
                    .strip_prefix(' ')
                    .unwrap_or(content)
                    .trim_end()
                    .to_owned(),
            );
            *index += 1;
        }
        return Ok(Kind::Quote(body));
    }
    if list_marker(line).is_some() {
        return parse_list(lines, index);
    }
    if Table::starts(line) {
        let mut body = Vec::new();
        while *index < lines.len() && Table::starts(lines[*index]) {
            body.push(lines[*index]);
            *index += 1;
        }
        return Ok(Kind::Table(Table::parse(&body, start + 1)?));
    }
    let mut body = vec![line.trim().to_owned()];
    *index += 1;
    while *index < lines.len() && !lines[*index].trim().is_empty() && !starts_block(lines[*index]) {
        body.push(lines[*index].trim().to_owned());
        *index += 1;
    }
    Ok(Kind::Paragraph(body))
}

fn parse_fence(
    lines: &[&str],
    index: &mut usize,
    character: char,
    count: usize,
) -> Result<Kind, Diagnostic> {
    let start = *index;
    *index += 1;
    while *index < lines.len() {
        let closing = lines[*index].trim_start();
        if closing.starts_with(character) {
            let length = closing.chars().take_while(|c| *c == character).count();
            if length >= count && closing[length..].trim().is_empty() {
                *index += 1;
                return Ok(Kind::Literal(lines[start..*index].join("\n")));
            }
        }
        *index += 1;
    }
    Err(error(start + 1, "unclosed code fence"))
}

fn parse_list(lines: &[&str], index: &mut usize) -> Result<Kind, Diagnostic> {
    let mut items: Vec<ListItem> = Vec::new();
    let mut levels = Vec::new();
    while *index < lines.len() && !lines[*index].trim().is_empty() {
        if let Some((indent, ordered, content)) = list_marker(lines[*index]) {
            if lines[*index][..indent].contains('\t') {
                return Err(error(*index + 1, "use spaces to indent lists"));
            }
            while levels.last().is_some_and(|last| *last > indent) {
                levels.pop();
            }
            if levels.last().is_none_or(|last| *last < indent) {
                levels.push(indent);
            }
            items.push(ListItem {
                indent: (levels.len() - 1) * 2,
                ordered,
                lines: vec![content.to_owned()],
            });
        } else {
            let indentation = lines[*index].len() - lines[*index].trim_start().len();
            let parent = levels.last().copied().unwrap_or(0);
            if indentation <= parent || starts_block(lines[*index]) {
                break;
            }
            if let Some(item) = items.last_mut() {
                item.lines.push(lines[*index].trim().to_owned());
            }
        }
        *index += 1;
    }
    Ok(Kind::List(items))
}

pub(crate) fn is_annotation(line: &str) -> bool {
    line.trim_start()
        .strip_prefix('@')
        .and_then(|body| body.split(char::is_whitespace).next())
        .is_some_and(|name| annotation_options(name).is_some())
}

fn annotation_options(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "document" => Some(&["width", "table", "wrap"]),
        "ol" => Some(&["width", "start"]),
        "ul" | "paragraph" | "quote" => Some(&["width"]),
        "table" => Some(&["style", "id", "align"]),
        _ => None,
    }
}

fn starts_block(line: &str) -> bool {
    is_annotation(line)
        || fence(line).is_some()
        || heading(line).is_some()
        || line.trim() == "---"
        || is_quote(line)
        || list_marker(line).is_some()
        || Table::starts(line)
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim_start();
    let count = line.bytes().take_while(|ch| *ch == b'#').count();
    ((1..=6).contains(&count) && line[count..].chars().next().is_none_or(char::is_whitespace))
        .then(|| (count, line[count..].trim()))
}

fn is_quote(line: &str) -> bool {
    let line = line.trim_start();
    line == ">" || line.starts_with("> ") || line.starts_with(">\t")
}

fn fence(line: &str) -> Option<(char, usize)> {
    let line = line.trim_start();
    let character = line.chars().next()?;
    if character != '`' && character != '~' {
        return None;
    }
    let count = line.chars().take_while(|ch| *ch == character).count();
    (count >= 3).then_some((character, count))
}

fn list_marker(line: &str) -> Option<(usize, bool, &str)> {
    let body = line.trim_start();
    let indent = line.len() - body.len();
    if matches!(body, "-" | "*" | "+") {
        return Some((indent, false, ""));
    }
    if let Some(content) = body
        .strip_prefix("- ")
        .or_else(|| body.strip_prefix("* "))
        .or_else(|| body.strip_prefix("+ "))
    {
        return Some((indent, false, content.trim()));
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && &body[digits..] == "." {
        return Some((indent, true, ""));
    }
    if digits > 0
        && let Some(content) = body[digits..].strip_prefix(". ")
    {
        return Some((indent, true, content.trim()));
    }
    None
}

fn parse_annotation(line: &str, number: usize) -> Result<Annotation, Diagnostic> {
    let text = line.trim();
    let body = text.strip_prefix('@').unwrap_or_default();
    let mut tokens = body.split_whitespace();
    let name = tokens
        .next()
        .ok_or_else(|| error(number, "missing annotation name"))?;
    let allowed = annotation_options(name)
        .ok_or_else(|| error(number, format!("unknown annotation '{name}'")))?;
    let mut options = BTreeMap::new();
    let mut cursor = line.len() - line.trim_start().len() + 1 + name.len();
    for token in tokens {
        let start = cursor + line[cursor..].find(token).unwrap_or(0);
        let column = line[..start].chars().count() + 1;
        cursor = start + token.len();
        let failure = |message: String| Diagnostic {
            line: number,
            column,
            message,
        };
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| failure(format!("expected key=value, found '{token}'")))?;
        if !allowed.contains(&key) {
            return Err(failure(format!("unsupported option '{key}' for {name}")));
        }
        if options.contains_key(key) {
            return Err(failure(format!("duplicate option '{key}'")));
        }
        let valid = match key {
            "width" => value == "off" || positive_number(value),
            "start" => positive_number(value) && value.parse::<u32>().is_ok(),
            "table" | "style" => Style::parse(value).is_some(),
            "id" => valid_id(value),
            "wrap" => value.strip_prefix("table:").is_some_and(valid_id),
            "align" => value
                .split(',')
                .all(|v| matches!(v, "left" | "right" | "center")),
            _ => false,
        };
        if !valid {
            return Err(failure(format!("invalid value '{value}' for {key}")));
        }
        options.insert(key.to_owned(), value.to_owned());
    }
    Ok(Annotation {
        name: name.to_owned(),
        options,
        line: number,
    })
}

fn positive_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<usize>().is_ok_and(|n| n > 0)
}

fn valid_id(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_')
}
