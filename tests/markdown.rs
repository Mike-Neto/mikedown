#![allow(
    clippy::tests_outside_test_module,
    clippy::unwrap_used,
    reason = "Integration tests assert formatted Markdown structure and rendered output"
)]

use mikedown::format;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

fn rendered(source: &str) -> (String, String) {
    let formatted = format(source).unwrap();
    assert_eq!(
        format(&formatted).unwrap(),
        formatted,
        "formatting must be idempotent"
    );
    let mut output = String::new();
    html::push_html(
        &mut output,
        Parser::new_ext(&formatted, Options::ENABLE_TASKLISTS),
    );
    (formatted, output)
}

fn checkbox_depths(source: &str) -> Vec<(usize, bool)> {
    let mut depth: usize = 0;
    let mut checkboxes = Vec::new();
    for event in Parser::new_ext(source, Options::ENABLE_TASKLISTS) {
        match event {
            Event::Start(Tag::List(_)) => depth += 1,
            Event::End(TagEnd::List(_)) => depth -= 1,
            Event::TaskListMarker(checked) => checkboxes.push((depth, checked)),
            Event::Start(_)
            | Event::End(_)
            | Event::Text(_)
            | Event::Code(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Rule => {}
        }
    }
    checkboxes
}

#[test]
fn mixed_checklists_render_real_nested_checkboxes_and_generated_labels() {
    let (formatted, output) = rendered(
        "@ol\n- Prepare\n  - [x] Gather materials\n  - Check quantities\n- [ ] Continue\n",
    );
    assert_eq!(
        formatted,
        "@ol\n- 1. Prepare\n  - [x] a. Gather materials\n  - b. Check quantities\n- [ ] 2. Continue\n"
    );
    assert_eq!(checkbox_depths(&formatted), [(2, true), (1, false)]);
    assert_eq!(output.matches("type=\"checkbox\"").count(), 2);
    assert!(output.contains("a. Gather materials"));
    assert!(output.contains("b. Check quantities"));
    assert!(!output.contains("<pre>"));
}

#[test]
fn nine_checklist_levels_render_as_nine_nested_markdown_lists() {
    let source = "@ol\n- [ ] Root\n  - [x] Alpha\n    - [X] Roman\n      - [ ] Greek\n        - [ ] Numeric\n          - [x] Alpha again\n            - [ ] Roman again\n              - [X] Greek again\n                - [ ] Numeric again\n";
    let (formatted, output) = rendered(source);
    assert_eq!(
        checkbox_depths(&formatted),
        [
            (1, false),
            (2, true),
            (3, true),
            (4, false),
            (5, false),
            (6, true),
            (7, false),
            (8, true),
            (9, false)
        ]
    );
    assert_eq!(output.matches("type=\"checkbox\"").count(), 9);
    assert!(output.contains("α. Greek"));
    assert!(output.contains("i. Roman again"));
    assert!(!output.contains("<pre>"));
}

#[test]
fn todo_is_maintained_by_mikedown_and_renders_fifteen_native_tasks() {
    let source = include_str!("../TODO.md");
    let (formatted, output) = rendered(source);
    assert_eq!(formatted, source);
    assert_eq!(checkbox_depths(&formatted), vec![(1, false); 15]);
    assert_eq!(output.matches("type=\"checkbox\"").count(), 15);
    assert!(!output.contains("<pre>"));
    let prose = formatted.split_whitespace().collect::<Vec<_>>().join(" ");
    for text in [
        "through edits.",
        "rendered preview.",
        "editor grammars.",
        "annotation completions.",
        "allowances.",
    ] {
        assert!(prose.contains(text));
    }
}

#[test]
fn wrapped_words_are_prose_instead_of_alphabetic_items() {
    for prefix in ["1. ", "[ ] 1. ", "- [ ] 1. ", "- [ ] "] {
        for word in [
            "edits.",
            "preview.",
            "grammars.",
            "completions.",
            "i. followed by prose",
            "α. followed by prose",
        ] {
            let source = format!(
                "@ol\n{prefix}Preserve\n{}{word}\n",
                " ".repeat(prefix.len())
            );
            let formatted = format(&source).unwrap();
            assert!(
                formatted.contains(&format!("Preserve {word}")),
                "{source:?}: {formatted}"
            );
            assert_eq!(format(&formatted).unwrap(), formatted);
        }
    }
}

#[test]
fn explicit_anchors_distinguish_nested_items_from_aligned_prose() {
    let formatted =
        format("@ol\n1. Parent\n   @item id=child\n   a. Child\n\nSee [](item:child).\n").unwrap();
    assert!(formatted.contains("  @item id=child\n  a. Child"));
    assert!(formatted.contains("[1.a](item:child)"));
    assert_eq!(format(&formatted).unwrap(), formatted);
}

#[test]
fn removing_the_last_checkbox_does_not_duplicate_wrapper_labels() {
    let (formatted, _) = rendered("@ol\n- [ ] 3. Task\n- 4. Ordinary\n");
    let changed = format(&formatted.replace("- [ ] ", "- ")).unwrap();
    assert_eq!(changed, "@ol\n3. Task\n4. Ordinary\n");
    assert_eq!(format(&changed).unwrap(), changed);
}

#[test]
fn escaped_label_like_body_text_remains_literal() {
    let (formatted, output) = rendered("@ol\n- [ ] 12\\. Body text\n");
    assert_eq!(formatted, "@ol\n- [ ] 1. 12\\. Body text\n");
    assert!(output.contains("1. 12. Body text"));
}
