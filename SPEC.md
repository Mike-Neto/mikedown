# Mikedown language specification

Version: **0.1**. Status: implemented experimental language.

## 1. Model and canonical form

A document is UTF-8 text containing an optional document annotation and a sequence
of blocks. The formatter rewrites source into readable plain text. No separate
rendered representation is needed, and formatted output is valid editable source.

For every accepted source `s`, `format(format(s)) == format(s)`.

Canonical output uses LF newlines, one blank line between blocks, no leading or
trailing blank lines, and one final newline for nonempty documents. Empty input
formats to the empty string. The document annotation is separated from its first
block by one blank line; a block annotation sits immediately above its target.
Annotation options are emitted in alphabetical key order. Explicit options remain
present even when equal to defaults. Blank lines inside code fences are preserved.

Structural syntax uses ASCII spaces. Tabs may occur in prose and literal code,
but are invalid in list indentation. Formatted layout assumes a monospace font.
Display width is measured with `unicode-width` 0.2.2's non-CJK `width()` rules:
ambiguous characters occupy one column, wide characters generally occupy two,
and combining marks and emoji sequences use the crate's string-level rules.
Actual glyph widths can vary between fonts and terminals. The formatter never
normalizes Unicode characters or strips variation selectors.

## 2. Annotations

```text
annotation = "@" name *(whitespace key "=" value)
name = "document" | "paragraph" | "quote" | "ol" | "ul" | "table"
```

An annotation occupies its own line. Leading/trailing whitespace is accepted and
removed. No closing delimiter is used. Names, keys, and supported values are
case-sensitive. A known name must end at whitespace or end-of-line: `@ol` is a
directive, but `@olivia`, `@ol,`, and `@ol=3` are ordinary prose. Unknown names,
including `@mentions` and misspellings such as `@tabl`, remain ordinary prose.
Annotations are recognized only at the beginning of a line, after optional
indentation. Inline occurrences remain text.

Values are unquoted tokens without whitespace. Repeated keys, unsupported keys,
missing values, and invalid values on a recognized directive are errors. Escape
a known directive as `\@ol` to keep it as prose. The backslash remains visible,
since source is the display representation. Annotations inside code fences are
literal text.

A bare `---` remains a horizontal rule. The former `--- ol ---` syntax is ordinary
prose and is not recognized as an annotation.

Only one document annotation is allowed, before every content block. A block
annotation applies to the next block, even across blank lines. It cannot be
followed by a second annotation or end-of-file without a target. An incompatible
target is an error. Per-block options override document defaults. Nested list
items do not take independent annotations in v0.1.

| Annotation | Target | Supported options |
| --- | --- | --- |
| `@document` | Whole document | `width`, `table`, `wrap` |
| `@paragraph` | Next paragraph | `width` |
| `@quote` | Next quote block | `width` |
| `@ol` | Next list, converting its root items to numbers | `width`, `start` |
| `@ul` | Next list, converting its root items to bullets | `width` |
| `@table` | Next table | `style`, `align`, `id` |

Option values:

- `width`: positive integer display columns, or `off` to preserve physical prose
  line boundaries. Document default: `80`; blocks inherit the effective document
  width. Width is a target, not a hard maximum for indivisible content.
- `table` / `style`: `unicode`, `ascii`, or `pipes`. Document default: `unicode`.
- `start`: positive decimal integer up to 4294967295. Default: `1`. Applies to
  root ordered items; nested ordered lists start at one.
- `align`: comma-separated `left`, `right`, or `center`, exactly one per column.
  For example, `align=left,right`. Default: left, unless a pipe header separator
  supplies alignment. Right/center alignment inferred from a separator is saved
  as a table annotation so conversion to a bordered style stays repeatable.
- `id`: an ASCII letter followed by ASCII letters, digits, hyphens, or underscores.
  IDs are unique across tables in a document.
- `wrap`: `table:ID`. This is a formatter-maintained reference to the widest table,
  not a request to pin a particular table. See section 5.

Example:

```text
@document width=100 table=ascii

@paragraph width=40
This paragraph has its own width.

@ol start=3
- First step
- Second step
```

## 3. Blocks

Blank lines separate blocks. Recognized block starts also interrupt ordinary
paragraphs without an intervening blank line. A block annotation is retained as
part of its target block. Recognition order is code fence, heading, horizontal
rule, quote, list, table, then paragraph.

### Paragraphs and inline text

Adjacent ordinary lines form one paragraph. By default the formatter joins them,
normalizes inter-word whitespace to one space, and greedily wraps at word
boundaries. Long words and URLs are not split. A local `width` replaces, rather
than merely limits, the inherited width.

Matching backtick runs protect inline-code spans, including their internal
spacing. A run closes only a run of the same length. Unclosed inline backticks
are preserved and protect text until the paragraph's end. Inline Markdown such
as emphasis or links has no special display semantics and keeps its spelling.

With `width=off`, physical paragraph, item, or quote line boundaries remain in
place. Structural prefixes and surrounding whitespace still normalize.

Wrapping must not introduce new structural blocks. When a prospective line
boundary would turn an inline token into a heading, annotation, list, fence, or
table, that token stays on the previous line; that line may exceed its target.
For example, wrapping cannot move an inline `@ol` to the beginning of a line.

### Headings and horizontal rules

Headings use one through six `#` characters followed by whitespace or end-of-line.
The formatter retains the level and normalizes spacing to `# Heading`. Heading
text is not wrapped. Underline-style headings are not special syntax in v0.1.
A standalone `---` remains a horizontal rule.

### Lists

Bullets accept `-`, `*`, or `+`; numbered items accept decimal digits followed by
`.`. A nonempty item has a space after its marker. A marker alone is an empty
item. Existing ordered numbers are regenerated sequentially from one, or from an
`@ol start=N` annotation. Bullet output always uses `-`.

Indentation establishes nested levels; each deeper indentation starts a level,
and dedenting returns to an earlier level or establishes a new sibling level.
Canonical output uses two spaces per level and removes root indentation.
Continuation lines align under the item's text after its generated marker.
Continuation lines must be indented beyond their current item and cannot begin
another block. A blank line ends the list; multi-paragraph items are deferred.

An `@ol` or `@ul` annotation converts root items only. Nested item types remain as
written and their ordered counters are independent. Task markers such as `[x]`
remain ordinary item text. Quotes and fenced code nested inside list items are
not part of the v0.1 list grammar; they start separate top-level blocks.

### Quotes

A quote consists of consecutive lines beginning with `>` followed by whitespace
or end-of-line. Output uses `> ` before content and `>` for an empty quoted line.
Empty quoted lines separate paragraphs within the quote. Text wraps with the
prefix included in the width. Nested quote interpretation is deferred; additional
`>` characters are ordinary quote content.

### Literal code

A code fence starts with at least three backticks or tildes, optionally followed
by an information string. A closing fence uses the same character, at least as
many repetitions, and no following non-whitespace text. Unclosed fences are
errors. Fence lines and their enclosed content are preserved, including spaces
and blank lines, except that newline encoding becomes LF. Annotations inside
fences are literal text. Indented code blocks are not special syntax.

## 4. Tables

Input can be simple pipe rows, ASCII-bordered rows, or Unicode-bordered rows.
Every row has outer `|` delimiters for pipes/ASCII, or `│` for Unicode. The first
content row is the header. All content rows must contain the same number of cells;
empty cells are allowed. Adjacent table lines belong to one table; separate two
tables with a blank line or annotation.

A pipe table may have a separator immediately after its header. Separator cells
are three or more hyphens with optional leading/trailing `:` for alignment. A
leading colon means left, a trailing colon means right, and both mean center.
An all-separator row is reserved syntax and cannot be the first row.

Outside inline code, a backslash escapes the next character for delimiter
recognition and remains in the saved text. Use `\|` or `\│` for literal delimiters.
Matching backtick runs also protect delimiters inside inline code. When converting
styles, the formatter escapes any otherwise unprotected delimiter inside a cell.
Cell text is preserved apart from trimming surrounding whitespace and introducing
these necessary escapes. No content is truncated and no cells are wrapped.

Column width is the maximum displayed width of its cells. Every cell receives one
space of padding on either side. Additional padding implements alignment; odd
center padding puts the extra space on the right. Pipe tables have minimum
content widths of three, four, and five columns for left, right, and center
alignment respectively, so separator rows always retain at least three hyphens.
Bordered tables have a minimum content width of one.

Styles:

```text
@table style=unicode
┌──────┬───────┐
│ Name │ Value │
├──────┼───────┤
│ Tea  │ 2     │
└──────┴───────┘

@table style=ascii
+------+-------+
| Name | Value |
+------+-------+
| Tea  | 2     |
+------+-------+

@table style=pipes
| Name | Value |
| ---- | ----- |
| Tea  | 2     |
```

Unicode uses `┌┬┐`, `├┼┤`, `└┴┘`, `─`, and `│`. ASCII uses `+`, `-`, and `|`.
Borders appear above the header, between header and body, and below the table.
For a header-only table the bottom border also closes the header. Pipe style
has only the header separator. Existing border widths are ignored on input so
cells can be edited directly; border character families are validated and every
border is regenerated. A table must have at least a header row.

## 5. Tables as document wrapping targets

Table layout never depends on prose wrapping. Formatting uses these passes:

1. Parse and validate every block and annotation, including unique table IDs.
2. Lay out all tables using their effective styles and alignments.
3. Measure each complete table, including borders and padding. Select the widest;
   the first in document order wins equal-width ties.
4. Compute `effective_width = max(configured_width, widest_table_width)`.
5. If the widest table exceeds the configured minimum, give it an ID if needed and
   add/update `wrap=table:ID` in the document annotation. Reflow inheriting prose,
   list items, and quotes to the effective width.

Generated IDs use the smallest available `table-N`, starting at one. Existing IDs
remain unchanged. If necessary, the formatter inserts the leading document
annotation and the target's table annotation, preserving existing options.

On every run, the widest table is reconsidered. Changing style, editing cell text,
adding/deleting tables, or removing the previous target can change the reference.
A syntactically valid stale reference is repaired automatically. If no table is
wider than the configured minimum, `wrap` is removed. Existing annotations and
IDs remain, even if no longer needed as targets. Duplicate IDs are errors.

If document `width=off`, prose is not reflowed and no wrapping reference is kept.
Explicit block widths, including `off`, override table-driven document width.
Headings and literal code are never reflowed.

This process has no dependency cycle, because tables are neither clipped nor
wrapped to the prose width. A second run therefore preserves both layout and IDs.

## 6. Library and CLI

The public Rust API is:

```rust
pub fn format(source: &str) -> Result<String, Vec<Diagnostic>>;
```

`Diagnostic` has public `line`, `column`, and `message` fields and implements
`Display` and `Error`. Coordinates are one-based; columns count Unicode scalars,
not UTF-8 bytes or display width. Structural errors point to the beginning of
their source line; option errors point to the invalid token. v0.1 returns the
first document diagnostic in the vector, without partial formatted output.

```text
mikedown fmt [FILE|-]
mikedown fmt --write FILE...
mikedown fmt --check FILE...
```

Omitted input or `-` reads stdin in stdout mode. Stdout mode accepts one input and
never writes it back. `--write` and `--check` require named paths and are mutually
exclusive. `--` ends option parsing. The CLI does not scan directories; shells
may expand globs. UTF-8 input is required; file extensions are not enforced.

Exit codes: `0` for success/already formatted, `1` for differences in check mode,
`2` for usage, parsing, UTF-8, or I/O errors. Check reports and diagnostics go to
stderr; formatted documents go only to stdout. `--help` and `--version` exit zero.

Write mode validates all inputs before changing any file, rejects duplicate
paths and symlinks, and stages all changed output in exclusive temporary files
in the destination directories. It preserves permissions, syncs temporary files,
and uses rename to replace each destination atomically. Unchanged files are not
replaced. Temporary files are cleaned up on ordinary errors. A batch is not a
filesystem-wide transaction: an I/O failure during the rename phase can occur
after earlier files have been replaced. Crash recovery and concurrent editing
coordination are outside v0.1.

## 7. Deferred capabilities

Markdown compatibility guarantees, HTML/PDF output, rendering, includes, macros,
project configuration files, generated contents lists, multi-paragraph list
items, multi-line table cells, and editor plugins are outside this version.
Plugin work and shared-core integration notes live in `TODO.md`.
