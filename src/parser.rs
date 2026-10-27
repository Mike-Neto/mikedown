use alloc::collections::BTreeMap;

use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while_m_n, take_while1};
use nom::character::complete::{char, digit1, one_of, satisfy};
use nom::combinator::{all_consuming, eof, map, map_res, peek, recognize, rest, value, verify};
use nom::multi::many0;
use nom::sequence::{preceded, terminated};
use nom::{IResult, Offset as _, Parser as _};

use crate::list::GREEK;
use crate::table::Table;
use crate::text::Text;
use crate::{Annotation, Diagnostic, error};

/// HTML tags that start block content under Markdown precedence rules.
pub const HTML_BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// Parsed blocks with optional document settings and preserved front matter.
pub struct Document {
    /// Settings attached to this document or block.
    pub annotation: Option<Annotation>,
    /// Content blocks in source order.
    pub blocks: Vec<Block>,
    /// Original front matter, including its boundary lines.
    pub front_matter: Option<String>,
}

/// A content block with its settings and original source location.
pub struct Block {
    /// Settings attached to this document or block.
    pub annotation: Option<Annotation>,
    /// Parsed content and layout category.
    pub kind: Kind,
    /// One-based source line of the block.
    pub line: usize,
}

/// Block categories recognized before formatting.
pub enum Kind {
    /// Heading level and source-located inline text.
    Heading(usize, Text),
    /// List items in source order, including nested items.
    List(Vec<ListItem>),
    /// Source content preserved without prose wrapping.
    Literal(String),
    /// Source-located prose lines.
    Paragraph(Vec<Text>),
    /// Quote body lines with their prefixes removed.
    Quote(Vec<Text>),
    /// Rectangular table cells.
    Table(Table),
}

/// An item with source metadata and generated numbering information.
pub struct ListItem {
    /// Optional item-ID annotation immediately preceding the marker.
    pub anchor: Option<Annotation>,
    /// Original checkbox state, when present.
    pub checkbox: Option<char>,
    /// Zero-based nesting depth after indentation normalization.
    pub depth: usize,
    /// Indentation width; normalized to two spaces per level for parsed items.
    pub indent: usize,
    /// Source-located body lines, excluding the list marker.
    pub lines: Vec<Text>,
    /// Canonical marker assigned during list numbering.
    pub marker: String,
    /// Whether the source marker uses ordered numbering.
    pub ordered: bool,
    /// Generated full ordinal path used by item references.
    pub path: String,
    /// Starting ordinal parsed from the source marker.
    pub start: usize,
}

/// Borrowed marker content before list indentation is normalized.
struct ParsedMarker<'source> {
    /// Original checkbox state, when present.
    checkbox: Option<char>,
    /// Body text following the marker and optional checkbox.
    content: &'source str,
    /// Original indentation width in bytes.
    indent: usize,
    /// Whether the source marker uses ordered numbering.
    ordered: bool,
    /// Starting ordinal parsed from the source marker.
    start: usize,
}

#[expect(
    clippy::too_many_lines,
    reason = "List parsing keeps continuation recognition inline"
)]
/// Consume consecutive list items and continuations, validating anchors and indentation.
pub fn parse_list(lines: &[&str], index: &mut usize, cycle: bool) -> Result<Kind, Diagnostic> {
    let mut items: Vec<ListItem> = Vec::new();
    // Each consumed line bounds the cursor; nesting levels are nonempty before normalization.
    let mut levels = Vec::new();
    while let Some(&current) = lines.get(*index).filter(|line| !line.trim().is_empty()) {
        if is_rule(current) {
            break;
        }
        let anchor = if item_annotation(current) {
            if !cycle {
                return Err(error(
                    index.strict_add(1),
                    "@item is only valid inside an @ol list",
                ));
            }
            let annotation = parse_annotation(current, index.strict_add(1))?;
            if !annotation.options.contains_key("id") {
                return Err(error(index.strict_add(1), "@item requires id=NAME"));
            }
            let indent = current.len().strict_sub(current.trim_start().len());
            if current.split_at(indent).0.contains('\t') {
                return Err(error(index.strict_add(1), "use spaces to indent lists"));
            }
            if lines
                .get(index.strict_add(1))
                .filter(|line| !is_rule(line))
                .and_then(|line| cycle_marker(line))
                .is_none_or(|(next_indent, _, _)| next_indent != indent)
            {
                return Err(error(
                    index.strict_add(1),
                    "@item must immediately precede an item at the same indentation",
                ));
            }
            *index = index.strict_add(1);
            Some(annotation)
        } else {
            None
        };
        let Some(&item_line) = lines.get(*index) else {
            break;
        };
        let marker = if cycle {
            cycle_syntax(item_line).ok().map(|(_, marker)| marker)
        } else {
            list_marker(item_line).map(|(indent, ordered, content)| ParsedMarker {
                indent,
                ordered,
                content,
                checkbox: None,
                start: source_number(item_line.trim_start()),
            })
        }
        .filter(|marker| {
            !cycle
                || anchor.is_some()
                || !{
                    // Aligned continuation.

                    let indent: usize = marker.indent;
                    let previous: Option<&ListItem> = items.last();

                    list_marker(item_line).is_none()
                        && checkbox_syntax(item_line.trim_start()).is_err()
                        && previous
                            .and_then(|item| item.lines.first())
                            .is_some_and(|body| {
                                // Empty markers omit the separator space that precedes nonempty body text.
                                let column = body
                                    .column
                                    .saturating_sub(usize::from(!body.value.is_empty()));
                                item_line.split_at(indent).0.chars().count() >= column
                            })
                }
        });
        if let Some(ParsedMarker {
            indent,
            ordered,
            content,
            checkbox,
            start,
        }) = marker
        {
            if item_line.split_at(indent).0.contains('\t') {
                return Err(error(index.strict_add(1), "use spaces to indent lists"));
            }
            while levels.last().is_some_and(|last| *last > indent) {
                levels.pop();
            }
            if levels.last().is_none_or(|last| *last < indent) {
                levels.push(indent);
            }
            items.push(ListItem {
                indent: levels.len().strict_sub(1).strict_mul(2),
                depth: levels.len().strict_sub(1),
                ordered,
                checkbox,
                start,
                lines: vec![located(content, item_line, index.strict_add(1))],
                anchor,
                marker: String::new(),
                path: String::new(),
            });
        } else {
            let indentation = item_line.len().strict_sub(item_line.trim_start().len());
            let parent = levels.last().copied().unwrap_or(0);
            if indentation <= parent || starts_block(item_line) {
                break;
            }
            if let Some(item) = items.last_mut() {
                item.lines.push(located(
                    item_line.trim_start(),
                    item_line,
                    index.strict_add(1),
                ));
            }
        }
        *index = index.strict_add(1);
    }
    Ok(Kind::List(items))
}

/// Attach the original one-based scalar column and line to borrowed inline content.
pub fn located(content: &str, original: &str, line: usize) -> Text {
    Text::at(
        content,
        line,
        original
            .split_at(original.offset(content))
            .0
            .chars()
            .count()
            .strict_add(1),
    )
}

/// Test whether a line begins an item annotation.
pub fn item_annotation(line: &str) -> bool {
    annotation_name(line).is_ok_and(|(_, name)| name == "item")
}

// Match the existing Unicode whitespace behavior rather than nom's ASCII space0.
/// Recognize zero or more Unicode whitespace scalars.
pub fn whitespace(input: &str) -> IResult<&str, &str> {
    take_while(char::is_whitespace).parse(input)
}

/// Recognize a nonempty annotation option token.
fn token(input: &str) -> IResult<&str, &str> {
    take_while1(|ch: char| !ch.is_whitespace()).parse(input)
}

/// Recognize a supported annotation name after its leading at sign.
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

/// Test whether a line starts a recognized annotation.
pub fn is_annotation(line: &str) -> bool {
    annotation_name(line).is_ok()
}

/// Return the allowed option names for a supported annotation.
fn annotation_options(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "document" => Some(&["mikedown", "width", "wrap"]),
        "ol" => Some(&["width", "start"]),
        "paragraph" | "quote" => Some(&["width"]),
        "table" | "item" => Some(&["id"]),
        _ => None,
    }
}

/// Test whether a line can start a block and interrupt prose.
pub fn starts_block(line: &str) -> bool {
    is_annotation(line)
        || html_start(line).is_some()
        || reference(line)
        || setext(line)
        || fence(line).is_some()
        || heading(line).is_some()
        || is_rule(line)
        || is_quote(line)
        || list_marker(line).is_some()
        || Table::starts(line)
}

/// Recognize an ATX heading and return its level and body.
pub fn heading(line: &str) -> Option<(usize, &str)> {
    {
        // Heading syntax.

        let input: &str = line;
        let result: IResult<&str, (usize, &str)> = 'heading_syntax: {
            let (input, marks) =
                match preceded(whitespace, take_while_m_n(1, 6, |ch| ch == '#')).parse(input) {
                    Ok(value) => value,
                    Err(failure) => break 'heading_syntax Err(failure),
                };
            let (input, _) =
                match peek(alt((recognize(satisfy(char::is_whitespace)), eof))).parse(input) {
                    Ok(value) => value,
                    Err(failure) => break 'heading_syntax Err(failure),
                };
            Ok(("", (marks.len(), input.trim())))
        };
        result
    }
    .ok()
    .map(|(_, heading)| heading)
}

/// Test for a tab or at least four leading spaces.
pub fn indented(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

/// Recognize a nonempty, unindented setext heading underline.
pub fn setext(line: &str) -> bool {
    !indented(line)
        && !line.trim().is_empty()
        && (line.trim().chars().all(|character| character == '=')
            || line.trim().chars().all(|character| character == '-'))
}

/// Recognize a Markdown link-reference definition.
pub fn reference(line: &str) -> bool {
    !indented(line) && line.trim_start().starts_with('[') && line.contains("]:")
}

/// Recognize an HTML block opener and its closing sentinel, if needed.
pub fn html_start(line: &str) -> Option<&'static str> {
    if indented(line) {
        return None;
    }
    let lower = line.trim_start().to_ascii_lowercase();
    let text = lower.as_str();
    if text.starts_with("<!--") {
        return Some("-->");
    }
    if text.starts_with("<?") {
        return Some("?>");
    }
    if text.starts_with("<![CDATA[") {
        return Some("]]>");
    }
    if text.starts_with("<!") {
        return Some(">");
    }
    for (tag, closer) in [
        ("script", "</script>"),
        ("pre", "</pre>"),
        ("style", "</style>"),
        ("textarea", "</textarea>"),
    ] {
        if text
            .strip_prefix(&format!("<{tag}"))
            .is_some_and(|tail| tail.is_empty() || tail.starts_with([' ', '\t', '>']))
        {
            return Some(closer);
        }
    }
    if let Some(tail) = text.strip_prefix('<') {
        let tail = tail.strip_prefix('/').unwrap_or(tail);
        let name = tail
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '-')
            .next()
            .unwrap_or("");
        if HTML_BLOCK_TAGS.contains(&name)
            && tail
                .split_at(name.len())
                .1
                .starts_with([' ', '\t', '>', '/'])
        {
            return Some("");
        }
        if !name.is_empty()
            && name.starts_with(|character: char| character.is_ascii_alphabetic())
            && text.ends_with('>')
            && !text.contains("://")
            && !text.contains('@')
            && tail
                .split_at(name.len())
                .1
                .starts_with([' ', '\t', '>', '/'])
        {
            return Some("");
        }
    }
    None
}

/// Test for a table header followed by a matching separator row.
pub fn table_at(lines: &[&str], index: usize) -> bool {
    lines.get(index..).is_some_and(|remaining| {
        remaining.first().is_some_and(|header| header.contains('|'))
            && remaining.get(1).is_some_and(|next| {
                // Separator.
                use crate::inline;
                use crate::table::separator_cell;
                let line: &str = next;

                let text = line.trim().trim_matches('|');
                let cells = inline::cells(text, '|');
                !cells.is_empty() && cells.iter().all(|cell| separator_cell(cell).is_ok())
            })
    })
}

/// Recognize a thematic break outside indented code.
pub fn is_rule(line: &str) -> bool {
    if indented(line) {
        return false;
    }
    let marks: String = line
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    marks.len() >= 3
        && ['-', '*', '_']
            .iter()
            .any(|mark| marks.chars().all(|character| character == *mark))
}

/// Recognize a quote prefix and return the remaining body.
pub fn quote(input: &str) -> IResult<&str, &str> {
    preceded(
        whitespace,
        preceded(
            char('>'),
            preceded(peek(alt((tag(" "), tag("\t"), eof))), rest),
        ),
    )
    .parse(input)
}

/// Test whether a line begins a quote block.
pub fn is_quote(line: &str) -> bool {
    quote(line).is_ok()
}

/// Recognize a code fence and return its character and run length.
pub fn fence(line: &str) -> Option<(char, usize)> {
    {
        // Fence syntax.

        let input: &str = line;
        let result: IResult<&str, (char, usize)> = 'fence_syntax: {
            let (input, character) = match peek(preceded(whitespace, one_of("`~"))).parse(input) {
                Ok(value) => value,
                Err(failure) => break 'fence_syntax Err(failure),
            };
            let (input, marks) = match preceded(
                whitespace,
                verify(take_while1(|ch| ch == character), |marks: &str| {
                    marks.len() >= 3
                }),
            )
            .parse(input)
            {
                Ok(value) => value,
                Err(failure) => break 'fence_syntax Err(failure),
            };
            let count = marks.len();
            Ok((input, (character, count)))
        };
        result
    }
    .ok()
    .map(|(_, fence)| fence)
}

/// Recognize an ordinary Markdown list marker and its indentation and body.
pub fn list_marker(line: &str) -> Option<(usize, bool, &str)> {
    {
        // List syntax.

        let input: &str = line;
        let result: IResult<&str, (usize, bool, &str)> = 'list_syntax: {
            let (input, indentation) = match whitespace(input) {
                Ok(value) => value,
                Err(failure) => break 'list_syntax Err(failure),
            };
            let (input, ordered) = match alt((
                value(false, one_of("-*+")),
                value(true, (digit1, char('.'))),
            ))
            .parse(input)
            {
                Ok(value) => value,
                Err(failure) => break 'list_syntax Err(failure),
            };
            let (input, content) = match alt((eof, preceded(char(' '), rest))).parse(input) {
                Ok(value) => value,
                Err(failure) => break 'list_syntax Err(failure),
            };
            Ok((input, (indentation.len(), ordered, content.trim_start())))
        };
        result
    }
    .ok()
    .map(|(_, marker)| marker)
}

/// Read a decimal source ordinal, defaulting to one for other markers.
fn source_number(input: &str) -> usize {
    input
        .split('.')
        .next()
        .and_then(|number| number.parse().ok())
        .unwrap_or(1)
}

/// Recognize a checkbox and return its state character.
fn checkbox_syntax(input: &str) -> IResult<&str, char> {
    let (input, _) = char('[').parse(input)?;
    let (input, state) = one_of(" xX").parse(input)?;
    let (input, _) = char(']').parse(input)?;
    let (input, _) = alt((eof, tag(" "))).parse(input)?;
    Ok((input.trim_start(), state))
}

/// Recognize an ordered marker and split its ordinal from its body.
fn ordered_content(input: &str) -> IResult<&str, (&str, &str)> {
    let (input, label) = terminated(
        alt((
            digit1,
            take_while1(|ch: char| ch.is_ascii_lowercase()),
            take_while1(|ch| GREEK.contains(ch)),
        )),
        char('.'),
    )
    .parse(input)?;
    let (input, content) = alt((eof, preceded(char(' '), rest))).parse(input)?;
    Ok((input, (label, content.trim_start())))
}

/// Recognize decimal, alphabetic, Roman, Greek, or checkbox cycle markers.
fn cycle_syntax(input: &str) -> IResult<&str, ParsedMarker<'_>> {
    let (input, indentation) = whitespace(input)?;
    if let Ok((body, state)) = checkbox_syntax(input) {
        let (start, content) = ordered_content(body)
            .ok()
            .map_or((1, body), |(_, (label, content))| {
                (source_number(label), content)
            });
        return Ok((
            "",
            ParsedMarker {
                indent: indentation.len(),
                ordered: true,
                start,
                checkbox: Some(state),
                content,
            },
        ));
    }
    let (remaining, (ordered, content)) = alt((
        map(ordered_content, |(_, content)| (true, content)),
        map(
            preceded(one_of("-*+"), alt((eof, preceded(char(' '), rest)))),
            |content: &str| (false, content.trim_start()),
        ),
    ))
    .parse(input)?;
    let (checkbox, content) = checkbox_syntax(content)
        .ok()
        .map_or((None, content), |(body, state)| (Some(state), body));
    let (start, content) = if ordered {
        (source_number(input), content)
    } else {
        ordered_content(content)
            .or_else(|failure| {
                if checkbox.is_none() {
                    // Escaped decimal wrapper labels remain numbering, not body text.
                    let (marker_rest, (label, _, body)) =
                        (digit1, tag("\\."), alt((eof, preceded(char(' '), rest))))
                            .parse(content)?;
                    Ok((marker_rest, (label, body.trim_start())))
                } else {
                    Err(failure)
                }
            })
            .ok()
            .map_or((1, content), |(_, (label, body))| {
                (source_number(label), body)
            })
    };
    Ok((
        remaining,
        ParsedMarker {
            indent: indentation.len(),
            ordered,
            start,
            checkbox,
            content,
        },
    ))
}

/// Return indentation, ordering, and body for a cycling-list marker.
pub fn cycle_marker(line: &str) -> Option<(usize, bool, &str)> {
    cycle_syntax(line)
        .ok()
        .map(|(_, marker)| (marker.indent, marker.ordered, marker.content))
}

/// Parse and validate annotation options with source-located diagnostics.
pub fn parse_annotation(line: &str, number: usize) -> Result<Annotation, Diagnostic> {
    let (remaining, name) =
        annotation_name(line).map_err(|_parse_error| error(number, "missing annotation name"))?;
    let allowed = annotation_options(name)
        .ok_or_else(|| error(number, format!("unknown annotation '{name}'")))?;
    let (_, tokens) = {
        // Annotation tokens.

        let input: &str = remaining;
        let result: IResult<&str, Vec<&str>> = {
            all_consuming(terminated(many0(preceded(whitespace, token)), whitespace)).parse(input)
        };
        result
    }
    .map_err(|_parse_error| error(number, "invalid annotation options"))?;
    let mut options = BTreeMap::new();
    let mut id_column = 1;
    for token in tokens {
        let column = line
            .split_at(line.offset(token))
            .0
            .chars()
            .count()
            .strict_add(1);
        let failure = |message: String| Diagnostic {
            line: number,
            column,
            message,
        };
        let (_, (key, value)) = {
            // Option pair.

            let input: &str = token;
            let result: IResult<&str, (&str, &str)> = 'option_pair: {
                let (input, key) =
                    match terminated(take_while(|ch| ch != '='), char('=')).parse(input) {
                        Ok(value) => value,
                        Err(parse_error) => break 'option_pair Err(parse_error),
                    };
                let (input, value) = match rest(input) {
                    Ok(value) => value,
                    Err(parse_error) => break 'option_pair Err(parse_error),
                };
                Ok((input, (key, value)))
            };
            result
        }
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
            "id" => valid_id(value),
            "wrap" => {
                // Wrap reference.

                let input: &str = value;
                let result: IResult<&str, &str> =
                    { all_consuming(preceded(tag("table:"), identifier)).parse(input) };
                result
            }
            .is_ok(),
            _ => false,
        };
        if !valid {
            return Err(failure(format!("invalid value '{value}' for {key}")));
        }
        if key == "id" {
            id_column = column;
        }
        options.insert(key.to_owned(), value.to_owned());
    }
    Ok(Annotation {
        name: name.to_owned(),
        options,
        line: number,
        id_column,
    })
}

/// Test whether a decimal value is a nonzero representable `usize`.
fn positive_number(value: &str) -> bool {
    {
        // Number.

        let input: &str = value;
        let result: IResult<&str, usize> = {
            all_consuming(verify(map_res(digit1, str::parse::<usize>), |number| {
                *number > 0
            }))
            .parse(input)
        };
        result
    }
    .is_ok()
}

/// Recognize an item or table ID.
fn identifier(input: &str) -> IResult<&str, &str> {
    recognize((
        satisfy(|ch| ch.is_ascii_alphabetic()),
        take_while(|ch: char| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'),
    ))
    .parse(input)
}

/// Test whether an entire value is a valid item or table ID.
pub fn valid_id(value: &str) -> bool {
    all_consuming(identifier).parse(value).is_ok()
}
