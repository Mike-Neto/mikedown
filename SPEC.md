@document width=120

# Mikedown language specification

Version: **0.1**. Status: implemented experimental language.

## 1. Model and canonical form

A document is UTF-8 text containing an optional document annotation and a sequence of blocks. Both `@document` and its
`mikedown` version option are optional. Use `@document mikedown=0.1.0` to declare the supported version explicitly. The
formatter preserves supplied declarations and never inserts a document annotation or version option automatically. Empty
input remains empty. The formatter rewrites source into readable plain text. No separate rendered representation is
needed, and formatted output is valid editable source.

For every accepted source `s`, `format(format(s)) == format(s)`.

Canonical output uses LF newlines, one blank line between blocks, no leading or trailing blank lines, and one final
newline for nonempty documents. Empty input formats to the empty string. The document annotation is separated from its
first block by one blank line; a block annotation sits immediately above its target. Annotation options are emitted in
alphabetical key order. Explicit options remain present even when equal to defaults. Blank lines inside code fences are
preserved.

Structural syntax uses ASCII spaces. Tabs may occur in prose and literal code, but are invalid in list indentation.
Formatted layout assumes a monospace font. Display width is measured with `unicode-width` 0.2.2's non-CJK `width()`
rules: ambiguous characters occupy one column, wide characters generally occupy two, and combining marks and emoji
sequences use the crate's string-level rules. Actual glyph widths can vary between fonts and terminals. The formatter
never normalizes Unicode characters or strips variation selectors.

## 2. Annotations

```text
annotation = "@" name *(whitespace key "=" value)
name = "document" | "paragraph" | "quote" | "ol" | "table" | "item"
```

An annotation occupies its own line. Leading/trailing whitespace is accepted and removed. No closing delimiter is used.
Names, keys, and supported values are case-sensitive. A known name must end at whitespace or end-of-line: `@ol` is a
directive, but `@olivia`, `@ol,`, and `@ol=3` are ordinary prose. Unknown names, including `@mentions` and misspellings
such as `@tabl`, remain ordinary prose. Annotations are recognized only at the beginning of a line, after optional
indentation. Inline occurrences remain text.

Values are unquoted tokens without whitespace. Repeated keys, unsupported keys, missing values, and invalid values on a
recognized directive are errors. Escape a known directive as `\@ol` to keep it as prose. The backslash remains visible,
since source is the display representation. Annotations inside code fences are literal text.

A bare `---` remains a horizontal rule. The former `--- ol ---` syntax is ordinary prose and is not recognized as an
annotation.

Only one document annotation is allowed, before every content block. A block annotation applies to the next block, even
across blank lines. It cannot be followed by a second annotation or end-of-file without a target. An incompatible target
is an error. Per-block options override document defaults. Nested list items do not take independent formatting
annotations. Inside `@ol`, `@item id=NAME` attaches an identity to the immediately following item at the same
indentation, without a blank line.

@table id=table-1
| Annotation   | Target                                     | Supported options                    |
| ------------ | ------------------------------------------ | ------------------------------------ |
| `@document`  | Whole document                             | `mikedown`, `width`, `table`, `wrap` |
| `@paragraph` | Next paragraph                             | `width`                              |
| `@quote`     | Next quote block                           | `width`                              |
| `@ol`        | Next list, recursively ordering every item | `width`, `start`                     |
| `@table`     | Next table                                 | `style`, `align`, `id`               |
| `@item`      | Next item inside an `@ol` list             | `id` (required)                      |

Option values:

@ol
1. `mikedown`: the formatter package version, currently exactly `0.1.0`. The option is optional; the formatter preserves
   a supported explicit declaration. Other versions, including malformed or empty values, are errors located at the
   option token; they are never silently rewritten. The option is supported only on `@document`.
2. `width`: positive integer display columns, or `off` to preserve physical prose line boundaries. Document default:
   `80`; blocks inherit the effective document width. Width is a target, not a hard maximum for indivisible content.
3. `table` / `style`: `unicode`, `ascii`, or `pipes`. Document default: `pipes`.
4. `start`: positive decimal integer up to 4294967295. Default: `1`. Applies to root ordered items; nested lists inside
   `@ol` restart at one beneath each new parent.
5. `align`: comma-separated `left`, `right`, or `center`, exactly one per column. For example, `align=left,right`.
   Default: left, unless a pipe header separator supplies alignment. Right/center alignment inferred from a separator is
   saved as a table annotation so conversion to a bordered style stays repeatable.
6. `id`: an ASCII letter followed by ASCII letters, digits, hyphens, or underscores. IDs are unique across tables and
   anchored list items in a document.
7. `wrap`: `table:ID`. This is a formatter-maintained reference to the widest table, not a request to pin a particular table.
   See section 5.

Example:

```text
@document mikedown=0.1.0 width=100 table=ascii

@paragraph width=40
This paragraph has its own width.

@ol start=3
- First step
- Second step
```

## 3. Blocks

Blank lines separate blocks. Recognized block starts also interrupt ordinary paragraphs without an intervening blank
line. A block annotation is retained as part of its target block. Recognition order is code fence, heading, horizontal
rule, quote, list, table, then paragraph.

### Paragraphs and inline text

Adjacent ordinary lines form one paragraph. By default the formatter joins them, normalizes inter-word whitespace to one
space, and greedily wraps at word boundaries. Long words and URLs are not split. A local `width` replaces, rather than
merely limits, the inherited width.

Matching backtick runs protect inline-code spans, including their internal spacing. A run closes only a run of the same
length. Unmatched backticks remain ordinary text. Complete links, images, reference links, autolinks, and code spans
protect their internal whitespace and spelling.

With `width=off`, physical paragraph, item, or quote line boundaries remain in place. Structural prefixes and
surrounding whitespace still normalize.

Wrapping must not introduce new structural blocks. When a prospective line boundary would turn an inline token into a
heading, annotation, list, fence, or table, that token stays on the previous line; that line may exceed its target. For
example, wrapping cannot move an inline `@ol` to the beginning of a line.

### Headings and horizontal rules

Headings use one through six `#` characters followed by whitespace or end-of-line. The formatter retains the level and
normalizes spacing to `# Heading`. Heading text is not wrapped. Setext headings retain their underline form, taking
precedence over thematic breaks. Thematic breaks of three or more matching `-`, `*`, or `_` marks, optionally spaced,
retain their spelling.

### Lists

Bullets accept `-`, `*`, or `+`; numbered items accept decimal digits followed by `.`. A nonempty item has a space after
its marker. A marker alone is an empty item. Existing ordered numbers are regenerated sequentially from each list's
first number, or from an `@ol start=N` annotation. Bullet output always uses `-`.

Indentation establishes nested levels; each deeper indentation starts a level, and dedenting returns to an earlier level
or establishes a new sibling level. Canonical output uses two spaces per level and removes root indentation.
Continuation lines align under the item's text after its generated marker. Continuation lines must be indented beyond
their current item and cannot begin another block. A blank line ends the list; multi-paragraph items are deferred.

An `@ol` annotation converts every item in its subtree, including bullets, to an ordered item. Its markers cycle by
nesting depth: decimal (`1.`), lowercase alphabetic (`a.`), lowercase Roman (`i.`), lowercase Greek (`α.`), then decimal
again. Each new nested sibling list restarts at one. Root numbering uses `start=N` when supplied, otherwise the first
item's existing decimal number or one for other markers. Alphabetic labels continue after `z` with `aa`; Greek labels
use the 24 lowercase letters without final sigma and continue after `ω` with `αα`. Roman labels use subtractive notation
and repeated `m` for values above 3999.

Inside `@ol`, bullet input remains accepted. Ordered input markers may be decimal numbers, lowercase ASCII letters
(including Roman numerals), or lowercase Greek letters, followed by `.` and a space or end-of-line. The formatter
regenerates their style and nested values. Bare alphabetic, Roman, or Greek markers aligned at or beyond the current
item's body column remain continuation prose, so wrapped words such as `edits.` are preserved. Use canonical two-space
nesting or an explicit bullet, numeric marker, checkbox, or item anchor to identify a new nested item. Wrapping never
introduces a structural marker at a new line's beginning.

Unannotated lists retain their original behavior: only decimal ordered markers are recognized, and nested numbering
preserves the first source number. Bullet lists need no annotation. The former `@ul` directive is ordinary prose under
the unknown-directive policy. Quotes and fenced code nested inside list items are not part of the v0.1 list grammar;
they start separate top-level blocks.

### Ordered checklists

Inside `@ol`, an item may begin with a checkbox: `[ ]`, `[x]`, or `[X]`. The formatter preserves its spelling and
completion state, placing the generated marker immediately after it. When any item in an `@ol` subtree has a checkbox,
every item gets a Markdown bullet wrapper, including ordinary items, so GitHub renders real checkboxes and nesting:

```text
@ol
- [ ] 1. Prepare
  - [x] a. Gather materials
  - b. Check quantities
- 2. Continue
```

Checkbox and ordinary items share the same numbering sequence, nesting cycle, and counter resets. Completion states are
independent: formatting never checks a parent because its children are checked. Continuation lines align beneath the
item text, after the checkbox and generated marker.

Bare input such as `[ ] Task` is accepted inside `@ol`, as are conventional items such as `- [ ] Task` and
`1. [x] Task`. Previous checkbox-first output such as `[ ] 1. Task` is also accepted. These normalize to the wrapped
form `- [ ] 1. Task`. An ordinal immediately after a bullet wrapper or its checkbox is structural numbering and is
regenerated; `start=N` still controls the root. For example, `- [ ] 12. Task` starts at twelve when no root start is
specified. Escape a label-like body prefix as `12\.` to keep it literal. Only one generated label is consumed, and an
empty checkbox item is valid. Unannotated lists retain their original interpretation of numeric body text.

A checkbox must be followed by an ASCII space or end-of-line. Other bracketed forms remain ordinary item text. Outside
`@ol`, checkbox text retains its original meaning and placement. Anchors attach to checkbox items in the same way as
other items, and generated references contain the ordinal path without the checkbox.

### Stable item references

Inside `@ol`, place `@item id=NAME` immediately above the item to identify, using exactly the item's indentation. IDs
are explicit, document-local, and remain unchanged when items move or their text is edited. They follow the same
character rules as table IDs and share the document-wide namespace. Missing IDs, duplicate IDs, and anchors without an
immediate item at the same indentation are errors.

```text
@ol start=3
3. Prepare
  @item id=validate-input
  a. Validate input
    i. Check fields
      α. Check names
        1. Check length

See [3.a](item:validate-input).
```

Write `[](item:NAME)` for a reference. Every inline link whose exact destination is `item:NAME` has its label replaced
with the target's full current path, without a trailing dot. Existing labels, including stale numbers, are refreshed on
every format. The stable ID, rather than that displayed path, identifies the target. References may precede their
targets and appear in paragraphs, headings, quotes, list text, or table cells. Missing targets, invalid IDs, and targets
that identify tables are source-located errors and prevent writes. Renaming an ID requires updating its references.

Code spans, fenced and indented code, front matter, literal HTML, images, reference definitions, and content inside
other links remain literal. Ordinary link destinations and author-written labels remain unchanged. Cross-document
references are outside this version.

### Quotes

A quote consists of consecutive lines beginning with `>` followed by whitespace or end-of-line. Output uses `> ` before
content and `>` for an empty quoted line. Empty quoted lines separate paragraphs within the quote. Text wraps with the
prefix included in the width. Nested quote interpretation is deferred; additional `>` characters are ordinary quote
content.

### Literal code

A code fence starts with at least three backticks or tildes, optionally followed by an information string. A closing
fence uses the same character, at least as many repetitions, and no following non-whitespace text. Unclosed fences are
errors. Fence lines and their enclosed content are preserved, including spaces and blank lines, except that newline
encoding becomes LF. Annotations inside fences are literal text. Top-level indented code is preserved; it cannot
interrupt a paragraph. HTML blocks, comments, and reference definitions retain their internal whitespace. Directives in
literal content remain literal.

## 4. Tables

Input can be simple pipe rows, ASCII-bordered rows, or Unicode-bordered rows. Legacy rows have outer `|` delimiters for
pipes/ASCII, or `│` for Unicode. GFM tables with a header and separator may omit outer pipes. The first content row is
the header. Normalized content rows must contain the same number of cells; empty cells are allowed. Adjacent table lines
belong to one table; separate two tables with a blank line or annotation.

A pipe table may have a separator immediately after its header. Separator cells are three or more hyphens with optional
leading/trailing `:` for alignment. A leading colon means left, a trailing colon means right, and both mean center. An
all-separator row is reserved syntax and cannot be the first row.

Outside inline code, a backslash escapes the next character for delimiter recognition and remains in the saved text. Use
`\|` or `\│` for literal delimiters. Matching backtick runs also protect delimiters inside inline code. When converting
styles, the formatter escapes literal delimiters inside cells, including code spans. Cell text is preserved apart from
trimming surrounding whitespace and introducing these necessary escapes. No content is truncated and no cells are
wrapped.

Column width is the maximum displayed width of its cells. Every cell receives one space of padding on either side.
Additional padding implements alignment; odd center padding puts the extra space on the right. Pipe tables have minimum
content widths of three, four, and five columns for left, right, and center alignment respectively, so separator rows
always retain at least three hyphens. Bordered tables have a minimum content width of one.

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

Unicode uses `┌┬┐`, `├┼┤`, `└┴┘`, `─`, and `│`. ASCII uses `+`, `-`, and `|`. Borders appear above the header, between
header and body, and below the table. For a header-only table the bottom border also closes the header. Pipe style has
only the header separator. Existing border widths are ignored on input so cells can be edited directly; border character
families are validated and every border is regenerated. A table must have at least a header row.

## 5. Tables as document wrapping targets

Table layout never depends on prose wrapping. Formatting uses these passes:

@ol
1. Parse and validate every block and annotation, including unique table and item IDs.
2. Assign list markers and full item paths, then resolve item references in all supported text.
3. Lay out all tables using their effective styles and alignments.
4. Measure each complete table, including borders and padding. Select the widest; the first in document order wins
   equal-width ties.
5. Compute `effective_width = max(configured_width, widest_table_width)`.
6. If the widest table exceeds the configured minimum, give it an ID if needed and add/update `wrap=table:ID` when a
   document annotation is present. Without one, derive the width directly without generating IDs or references. Reflow
   inheriting prose, list items, and quotes to the effective width.

Generated IDs use the smallest available `table-N`, starting at one, reserving both table and item IDs. Existing IDs
remain unchanged. When a document annotation is present, the formatter may insert the target's table annotation,
preserving existing options. Without a document annotation, the same width is derived on every run without adding a
document annotation or IDs for width references.

On every run, the widest table is reconsidered. Changing style, editing cell text, adding/deleting tables, or removing
the previous target can change the reference. A syntactically valid stale reference is repaired automatically. If no
table is wider than the configured minimum, `wrap` is removed. Existing annotations and IDs remain, even if no longer
needed as targets. Duplicate IDs are errors.

If document `width=off`, prose is not reflowed and no wrapping reference is kept. Explicit block widths, including
`off`, override table-driven document width. Headings and literal code are never reflowed.

This process has no dependency cycle, because tables are neither clipped nor wrapped to the prose width. A second run
therefore preserves both layout and IDs.

## 6. Library and CLI

The public Rust API is:

```rust
pub fn format(source: &str) -> Result<String, Vec<Diagnostic>>;
```

`Diagnostic` has public `line`, `column`, and `message` fields and implements `Display` and `Error`. Coordinates are
one-based; columns count Unicode scalars, not UTF-8 bytes or display width. Structural errors point to the beginning of
their source line; option errors point to the invalid token. v0.1 returns the first document diagnostic in the vector,
without partial formatted output.

```text
mikedown fmt [FILE|-]
mikedown fmt --write FILE...
mikedown fmt --check FILE...
```

Omitted input or `-` reads stdin in stdout mode. Stdout mode accepts one input and never writes it back. `--write` and
`--check` require named paths and are mutually exclusive. `--` ends option parsing. The CLI does not scan directories;
shells may expand globs. UTF-8 input is required. Both `.mkd` and `.md` documents use the same syntax and formatting
behavior; file extensions are not enforced.

Exit codes: `0` for success/already formatted, `1` for differences in check mode or runtime failures (document parsing,
UTF-8, I/O, or write validation), and `2` for command-line usage errors. Runtime failures use Rust's standard `Result`
reporting: `Error: ` followed by the error's debug representation. Check reports and diagnostics go to stderr; formatted
documents go only to stdout. `--help` and `--version` exit zero.

Write mode validates all inputs before changing any file, rejects duplicate paths and symlinks, and stages all changed
output in exclusive temporary files in the destination directories. It preserves permissions, syncs temporary files, and
uses rename to replace each destination atomically. Unchanged files are not replaced. Temporary files are cleaned up on
ordinary errors. A batch is not a filesystem-wide transaction: an I/O failure during the rename phase can occur after
earlier files have been replaced. Crash recovery and concurrent editing coordination are outside v0.1.

## 7. Deferred capabilities

Full CommonMark/GFM conformance, HTML/PDF output, rendering, includes, macros, project configuration files, generated
contents lists, multi-paragraph list items, multi-line table cells, and editor plugins are outside this version. Plugin
work and shared-core integration notes live in `TODO.md`.

## Markdown preservation milestone

The same grammar applies to `.mkd`, `.md`, and stdin. Selected core constructs follow
[CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/), with
[GFM pipe tables](https://github.github.com/gfm/#tables-extension-). This is a preservation milestone, not a full
conformance claim.

A YAML front-matter opener `---` must be the first physical line. Preserve it through a `---` or `...` closer, or EOF
when unclosed. YAML is never parsed or sorted. Front matter precedes an optional document directive.

Trailing-space and backslash hard breaks survive wrapping in paragraphs, quotes, and existing list continuations. Each
segment wraps independently; `width=off` preserves physical lines.

Pipe tables are the default, including when input uses borders. Set document `table=unicode`/`table=ascii` or table
`style=unicode`/`style=ascii` to retain bordered output. Inferred alignment stays in pipe separators; bordered output
receives alignment metadata when needed for round trips. Irregular unannotated tables remain verbatim; explicit `@table`
targets retain strict validation. Recursive quotes, multi-paragraph list items, code nested in lists, Obsidian syntax,
and rendering remain deferred.
