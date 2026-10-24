use std::collections::BTreeMap;

use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while_m_n, take_while1};
use nom::character::complete::{char, digit1, one_of, satisfy};
use nom::combinator::{all_consuming, eof, map_res, peek, recognize, rest, value, verify};
use nom::multi::many0;
use nom::sequence::{preceded, terminated};
use nom::{IResult, Offset, Parser};

use crate::table::{Alignment, Style, Table};
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
    if is_rule(line) {
        *index += 1;
        return Ok(Kind::Literal("---".into()));
    }
    if is_quote(line) {
        let mut body = Vec::new();
        while *index < lines.len() && is_quote(lines[*index]) {
            let content = quote(lines[*index]).map_or("", |(_, content)| content);
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
        if closing_fence(lines[*index], character, count).is_ok() {
            *index += 1;
            return Ok(Kind::Literal(lines[start..*index].join("\n")));
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

// Match the existing Unicode whitespace behavior rather than nom's ASCII space0.
fn whitespace(input: &str) -> IResult<&str, &str> {
    take_while(char::is_whitespace).parse(input)
}

fn token(input: &str) -> IResult<&str, &str> {
    take_while1(|ch: char| !ch.is_whitespace()).parse(input)
}

fn annotation_name(input: &str) -> IResult<&str, &str> {
    preceded(
        whitespace,
        preceded(
            char('@'),
            verify(token, |name: &str| annotation_options(name).is_some()),
        ),
    )
    .parse(input)
}

pub(crate) fn is_annotation(line: &str) -> bool {
    annotation_name(line).is_ok()
}

fn annotation_options(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "document" => Some(&["mikedown", "width", "table", "wrap"]),
        "ol" => Some(&["width", "start"]),
        "ul" | "paragraph" | "quote" => Some(&["width"]),
        "table" => Some(&["style", "id", "align"]),
        _ => None,
    }
}

pub(crate) fn starts_block(line: &str) -> bool {
    is_annotation(line)
        || fence(line).is_some()
        || heading(line).is_some()
        || is_rule(line)
        || is_quote(line)
        || list_marker(line).is_some()
        || Table::starts(line)
}

fn heading_syntax(input: &str) -> IResult<&str, (usize, &str)> {
    let (input, marks) = preceded(whitespace, take_while_m_n(1, 6, |ch| ch == '#')).parse(input)?;
    let (input, _) = peek(alt((recognize(satisfy(char::is_whitespace)), eof))).parse(input)?;
    Ok(("", (marks.len(), input.trim())))
}

fn heading(line: &str) -> Option<(usize, &str)> {
    heading_syntax(line).ok().map(|(_, heading)| heading)
}

fn is_rule(line: &str) -> bool {
    fn rule(input: &str) -> IResult<&str, &str> {
        all_consuming(preceded(whitespace, terminated(tag("---"), whitespace))).parse(input)
    }
    rule(line).is_ok()
}

fn quote(input: &str) -> IResult<&str, &str> {
    preceded(
        whitespace,
        preceded(
            char('>'),
            preceded(peek(alt((tag(" "), tag("\t"), eof))), rest),
        ),
    )
    .parse(input)
}

fn is_quote(line: &str) -> bool {
    quote(line).is_ok()
}

fn fence_syntax(input: &str) -> IResult<&str, (char, usize)> {
    let (input, character) = peek(preceded(whitespace, one_of("`~"))).parse(input)?;
    let (input, marks) = preceded(
        whitespace,
        verify(take_while1(|ch| ch == character), |marks: &str| {
            marks.len() >= 3
        }),
    )
    .parse(input)?;
    let count = marks.len();
    Ok((input, (character, count)))
}

fn fence(line: &str) -> Option<(char, usize)> {
    fence_syntax(line).ok().map(|(_, fence)| fence)
}

fn closing_fence(input: &str, character: char, count: usize) -> IResult<&str, &str> {
    all_consuming(preceded(
        whitespace,
        terminated(
            verify(take_while1(|ch| ch == character), |marks: &str| {
                marks.len() >= count
            }),
            whitespace,
        ),
    ))
    .parse(input)
}

fn list_syntax(input: &str) -> IResult<&str, (usize, bool, &str)> {
    let (input, indentation) = whitespace(input)?;
    let (input, ordered) = alt((
        value(false, one_of("-*+")),
        value(true, (digit1, char('.'))),
    ))
    .parse(input)?;
    let (input, content) = alt((eof, preceded(char(' '), rest))).parse(input)?;
    Ok((input, (indentation.len(), ordered, content.trim())))
}

fn list_marker(line: &str) -> Option<(usize, bool, &str)> {
    list_syntax(line).ok().map(|(_, marker)| marker)
}

fn option_pair(input: &str) -> IResult<&str, (&str, &str)> {
    let (input, key) = terminated(take_while(|ch| ch != '='), char('=')).parse(input)?;
    let (input, value) = rest(input)?;
    Ok((input, (key, value)))
}

fn annotation_tokens(input: &str) -> IResult<&str, Vec<&str>> {
    all_consuming(terminated(many0(preceded(whitespace, token)), whitespace)).parse(input)
}

fn parse_annotation(line: &str, number: usize) -> Result<Annotation, Diagnostic> {
    let (remaining, name) =
        annotation_name(line).map_err(|_parse_error| error(number, "missing annotation name"))?;
    let allowed = annotation_options(name)
        .ok_or_else(|| error(number, format!("unknown annotation '{name}'")))?;
    let (_, tokens) = annotation_tokens(remaining)
        .map_err(|_parse_error| error(number, "invalid annotation options"))?;
    let mut options = BTreeMap::new();
    for token in tokens {
        let column = line[..line.offset(token)].chars().count() + 1;
        let failure = |message: String| Diagnostic {
            line: number,
            column,
            message,
        };
        let (_, (key, value)) = option_pair(token)
            .map_err(|_parse_error| failure(format!("expected key=value, found '{token}'")))?;
        if !allowed.contains(&key) {
            return Err(failure(format!("unsupported option '{key}' for {name}")));
        }
        if options.contains_key(key) {
            return Err(failure(format!("duplicate option '{key}'")));
        }
        if key == "mikedown" && value != crate::DOCUMENT_VERSION {
            return Err(failure(format!(
                "unsupported Mikedown version '{value}'; supported version is {}",
                crate::DOCUMENT_VERSION,
            )));
        }
        let valid = match key {
            "mikedown" => true,
            "width" => value == "off" || positive_number(value),
            "start" => positive_number(value) && value.parse::<u32>().is_ok(),
            "table" | "style" => Style::parse(value).is_some(),
            "id" => valid_id(value),
            "wrap" => wrap_reference(value).is_ok(),
            "align" => Alignment::parse_list(value).is_some(),
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

fn number(input: &str) -> IResult<&str, usize> {
    all_consuming(verify(map_res(digit1, str::parse::<usize>), |number| {
        *number > 0
    }))
    .parse(input)
}

fn positive_number(value: &str) -> bool {
    number(value).is_ok()
}

fn identifier(input: &str) -> IResult<&str, &str> {
    recognize((
        satisfy(|ch| ch.is_ascii_alphabetic()),
        take_while(|ch: char| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'),
    ))
    .parse(input)
}

fn valid_id(value: &str) -> bool {
    all_consuming(identifier).parse(value).is_ok()
}

fn wrap_reference(input: &str) -> IResult<&str, &str> {
    all_consuming(preceded(tag("table:"), identifier)).parse(input)
}
