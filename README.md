@document mikedown=0.1.0 table=pipes width=80

# Mikedown

Plain text is both the storage format and the presentation.

Mikedown puts the layout directly into the document: the numbering, line breaks,
spacing, and table borders are characters in the file. Open it in a text editor
or terminal and the presentation is already there. Reading it requires no
Mikedown-specific viewer.

## The file is the view

Here is a saved Mikedown document. Everything inside this block, including the
layout, is text in the file:

```text
# Morning routine

A quiet start to the day.

@ol
1. Make coffee
  a. Grind the beans
  b. Heat the water
2. Plan the day

@table style=unicode
┌────────┬───────┐
│ Drink  │ Price │
├────────┼───────┤
│ Coffee │ 3     │
│ Tea    │ 2     │
└────────┴───────┘
```

The small annotations are visible too. They carry the document's formatting
choices alongside its content, so those choices travel with the file.

## Readable beyond the tool

A document should remain useful wherever its text can be read. Notes, plans, and
technical documents can be opened in an ordinary editor, shared as files, or
read in a terminal. Their changes can be reviewed as text in version control.
The saved layout stays visible even when Mikedown is not installed.

This makes plain text a place for the finished document. The table has its
borders, the list has its numbers, and the paragraphs have their line breaks
before anyone opens a preview.

## Formatting is maintenance

The formatter handles the mechanical work: wrapping prose, aligning tables,
numbering nested lists, and keeping references consistent when items move. Those
updates are saved into the same text that people read.

The result remains editable input. Change a table cell, insert a list item, or
revise a paragraph directly in the document; formatting brings the layout back
into order. Running it again without edits leaves the document unchanged.

## Project status

Mikedown is experimental. The formatter CLI and Rust library are implemented;
Zed and Obsidian integrations are planned. Documents can use `.mkd` or `.md`.
Mikedown is an independent language with support for selected Markdown syntax.

Explore the [example document](examples/tour.mkd) to see more of the format. The
[language specification](SPEC.md) covers syntax, formatting, and CLI usage. The
[roadmap](TODO.md) describes what comes next, and the
[contributing guide](CONTRIBUTING.md) covers development.
