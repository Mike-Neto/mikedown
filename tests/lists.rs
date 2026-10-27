#![allow(
    clippy::tests_outside_test_module,
    clippy::unwrap_used,
    clippy::panic,
    reason = "Integration assertions report invalid formatting and source diagnostics"
)]

use mikedown::format;
use unicode_width::UnicodeWidthStr;

fn stable(source: &str) -> String {
    let output = format(source).unwrap_or_else(|errors| panic!("{source:?}: {errors:?}"));
    assert_eq!(
        format(&output).unwrap(),
        output,
        "formatting must be idempotent"
    );
    output
}

#[test]
fn ordered_subtrees_cycle_twice_and_reset_beneath_new_parents() {
    let source = "@ol start=3\n9. root\n  7. alpha\n    8. roman\n      9. greek\n        8. numeric\n          7. alpha again\n            6. roman again\n              5. greek again\n                4. numeric again\n  9. next alpha\n    9. restarted roman\n8. next root\n  9. restarted alpha\n";
    assert_eq!(
        stable(source),
        "@ol start=3\n3. root\n  a. alpha\n    i. roman\n      α. greek\n        1. numeric\n          a. alpha again\n            i. roman again\n              α. greek again\n                1. numeric again\n  b. next alpha\n    i. restarted roman\n4. next root\n  a. restarted alpha\n"
    );
}

#[test]
fn annotated_lists_convert_all_bullets_and_preserve_root_starts() {
    assert_eq!(
        stable("@ol\n7. root\n  - child\n    + grandchild\n      * great grandchild\n9. next\n"),
        "@ol\n7. root\n  a. child\n    i. grandchild\n      α. great grandchild\n8. next\n"
    );
    assert_eq!(
        stable("@ol\n- root\n  -\n    -\n"),
        "@ol\n1. root\n  a.\n    i.\n"
    );
    assert_eq!(
        stable("@ol\na. root\n  iii. child\n    α. grandchild\n"),
        "@ol\n1. root\n  a. child\n    i. grandchild\n"
    );
}

#[test]
fn ordinary_markdown_lists_and_alphabetic_prose_keep_their_meaning() {
    let source = "7. root\n  4. child\n  5. child two\n8. next\n\na. Alphabetic prose\ni. Roman prose\nα. Greek prose\n";
    assert_eq!(
        stable(source),
        "7. root\n  4. child\n  5. child two\n8. next\n\na. Alphabetic prose i. Roman prose α. Greek prose\n"
    );
    assert_eq!(
        stable("@ul\n1. root\n  7. child\n  9. next\n"),
        "@ul\n\n1. root\n  7. child\n  8. next\n"
    );
}

#[test]
fn alphabetic_and_greek_numbering_continue_past_their_alphabets() {
    let alpha = stable(&format!("@ol\n1. root\n{}", "  1. child\n".repeat(27)));
    assert!(alpha.contains("  z. child\n  aa. child\n"));
    let greek = stable(&format!(
        "@ol\n1. root\n  1. alpha\n    1. roman\n{}",
        "      1. child\n".repeat(25)
    ));
    assert!(greek.contains("      ρ. child\n      σ. child\n      τ. child\n"));
    assert!(greek.ends_with("      ω. child\n      αα. child\n"));
}

#[test]
fn roman_numbering_uses_subtraction_and_unbounded_thousands() {
    let output = stable(&format!(
        "@ol\n1. root\n  1. alpha\n{}",
        "    1. child\n".repeat(4000)
    ));
    for marker in ["iv", "ix", "xl", "xc", "cd", "cm", "mmmcmxcix", "mmmm"] {
        assert!(output.contains(&format!("    {marker}. child\n")));
    }
}

#[test]
fn cycle_markers_cannot_be_introduced_by_wrapping() {
    for marker in ["a.", "aa.", "iii.", "α.", "αα.", "@item"] {
        let source = format!("@ol width=10\n- root\n  - hello {marker} words follow\n");
        let output = stable(&source);
        assert!(output.contains(&format!("hello {marker}")));
    }
}

#[test]
fn unicode_markers_wrap_at_display_width_and_preserve_hard_breaks() {
    let output = stable(
        "@ol width=18\n- root\n  - alpha\n    - roman\n      - one two three four five six\n",
    );
    assert!(output.contains("      α. one two\n         three\n"));
    assert!(output.lines().all(|line| line.width() <= 18));
    assert_eq!(
        stable("@ol width=off\n- root\n  - one   two  \n    three\\\n    four\n"),
        "@ol width=off\n1. root\n  a. one   two  \n     three\\\n     four\n"
    );
}

#[test]
fn forward_and_backward_item_references_use_full_paths_in_all_inline_contexts() {
    let source = "# See [](item:leaf)\n\nSee [old](item:root).\n\n> See [](item:child).\n\n@ol start=3\n@item id=root\n- Root refers to [](item:child)\n  @item id=child\n  - Child refers to [](item:root)\n    @item id=leaf\n    - Leaf\n\n| Target |\n| [](item:leaf) |\n";
    let output = stable(source);
    for text in [
        "# See [3.a.i](item:leaf)",
        "See [3](item:root).",
        "> See [3.a](item:child).",
        "3. Root refers to [3.a](item:child)",
        "  a. Child refers to [3](item:root)",
        "| [3.a.i](item:leaf)",
    ] {
        assert!(output.contains(text), "missing {text:?}: {output}");
    }
    assert!(output.contains("  @item id=child\n  a."));
}

#[test]
fn references_follow_insertions_reordering_reparenting_and_start_changes() {
    for (body, path, changed_path) in [
        ("- Parent\n  @item id=target\n  - Target\n", "1.a", "9.a"),
        (
            "- Inserted root\n- Parent\n  - Inserted child\n  @item id=target\n  - Target\n",
            "2.b",
            "10.b",
        ),
        ("@item id=target\n- Target\n- Parent\n", "1", "9"),
        (
            "- Parent\n  - Child\n    @item id=target\n    - Target\n",
            "1.a.i",
            "9.a.i",
        ),
    ] {
        let output = stable(&format!("See [stale](item:target).\n\n@ol\n{body}"));
        assert!(output.starts_with(&format!("See [{path}](item:target).")));
        assert_eq!(output.matches("@item id=target").count(), 1);
        let changed = stable(&output.replace("@ol\n", "@ol start=9\n"));
        assert!(changed.starts_with(&format!("See [{changed_path}](item:target).")));
    }
    let output =
        stable("See [](item:kept).\n\n@ol\n- Removed before formatting\n@item id=kept\n- Kept\n");
    let changed = stable(&output.replace("1. Removed before formatting\n", ""));
    assert!(changed.starts_with("See [2](item:kept)."));
    let reset = stable(&changed.replace("@ol\n", "@ol start=1\n"));
    assert!(reset.starts_with("See [1](item:kept)."));
}

#[test]
fn references_leave_literal_content_images_and_other_links_untouched() {
    let source = "---\nref: [](item:missing)\n...\n\n`[](item:missing)` and ![](item:missing) and \\[](item:missing).\n\n[outer [](item:missing)](https://example.com)\n\n```text\n@item id=duplicate\n[](item:missing)\n```\n\n    @item id=duplicate\n    [](item:missing)\n\n<div>\n[](item:missing)\n</div>\n\n[label]: item:missing\n";
    assert_eq!(stable(source), source);
    let multiline = "A `code span\n[](item:missing)` remains literal.\n";
    assert!(stable(multiline).contains("[](item:missing)"));
}

#[test]
fn multiline_reference_labels_are_replaced_before_wrapping() {
    let source = "See [old\nlabel](item:target).\n\n@ol\n@item id=target\n- Target\n";
    assert!(stable(source).starts_with("See [1](item:target)."));
}

#[test]
fn anchors_require_valid_ids_immediate_targets_and_matching_indentation() {
    for (source, message) in [
        ("@item id=x\n1. item\n", "only valid inside an @ol"),
        ("@ol\n@item\n- item\n", "requires id=NAME"),
        ("@ol\n@item id=1bad\n- item\n", "invalid value"),
        ("@ol\n@item id=x\n", "immediately precede"),
        ("@ol\n@item id=x\n\n- item\n", "immediately precede"),
        ("@ol\n@item id=x\n  - item\n", "same indentation"),
        (
            "@ol\n@item id=x\n@item id=y\n- item\n",
            "immediately precede",
        ),
        ("@ol\n@item id=x\nparagraph\n", "immediately precede"),
        ("@ol\n@item id=x\n- - -\n", "immediately precede"),
        ("@ol\n- root\n\t@item id=x\n\t- item\n", "use spaces"),
    ] {
        let diagnostic = format(source).unwrap_err().remove(0);
        assert!(
            diagnostic.message.contains(message),
            "{source:?}: {diagnostic}"
        );
    }
}

#[test]
fn thematic_breaks_keep_precedence_over_ordered_list_conversion() {
    let diagnostic = format("@ol\n- - -\n").unwrap_err().remove(0);
    assert!(diagnostic.message.contains("incompatible target block"));
    assert_eq!(stable("@ol\n- First\n- - -\n"), "@ol\n1. First\n\n- - -\n");
}

#[test]
fn ordered_checkboxes_normalize_all_input_forms_and_share_ordinals() {
    assert_eq!(
        stable("@ol\n- [ ] One\n1. [x] Two\n[X] Three\n4. Plain\n"),
        "@ol\n- [ ] 1. One\n- [x] 2. Two\n- [X] 3. Three\n- 4. Plain\n"
    );
    assert_eq!(
        stable("@ol\n[ ] 7. One\n[x] 9. Two\n"),
        "@ol\n- [ ] 7. One\n- [x] 8. Two\n"
    );
    assert_eq!(
        stable("@ol start=3\n[ ] 7. One\n[x] 9. Two\n"),
        "@ol start=3\n- [ ] 3. One\n- [x] 4. Two\n"
    );
    assert_eq!(
        stable("@ol\n[ ]\n[x] 5.\n[X]\n"),
        "@ol\n- [ ] 1.\n- [x] 2.\n- [X] 3.\n"
    );
}

#[test]
fn checkbox_subtrees_preserve_states_across_cycles_and_parent_changes() {
    let source = "@ol\n- [ ] Root\n  - [x] Alpha\n    - [X] Roman\n      - [ ] Greek\n        - [ ] Numeric\n          - [x] Alpha again\n            - [ ] Roman again\n              - [X] Greek again\n                - [ ] Numeric again\n  - Other alpha\n    - [ ] Restarted roman\n- Other root\n  - [x] Restarted alpha\n";
    assert_eq!(
        stable(source),
        "@ol\n- [ ] 1. Root\n  - [x] a. Alpha\n    - [X] i. Roman\n      - [ ] α. Greek\n        - [ ] 1. Numeric\n          - [x] a. Alpha again\n            - [ ] i. Roman again\n              - [X] α. Greek again\n                - [ ] 1. Numeric again\n  - b. Other alpha\n    - [ ] i. Restarted roman\n- 2. Other root\n  - [x] a. Restarted alpha\n"
    );
}

#[test]
fn checkbox_prefixes_align_wrapping_across_digit_and_unicode_boundaries() {
    assert_eq!(
        stable("@ol start=9 width=20\n[ ] one two three four five\n[x] six seven eight nine ten\n"),
        "@ol start=9 width=20\n- [ ] 9. one two\n         three four\n         five\n- [x] 10. six seven\n          eight nine\n          ten\n"
    );
    let output = stable(
        "@ol width=22\n- [ ] Root\n  - [ ] Alpha\n    - [ ] Roman\n      - [ ] one two three four five\n",
    );
    assert!(output.contains("      - [ ] α. one two\n               three\n"));
    assert!(output.lines().all(|line| line.width() <= 22));
    assert_eq!(
        stable("@ol width=off\n- [ ] one   two  \n    three\\\n    four\n"),
        "@ol width=off\n- [ ] 1. one   two  \n         three\\\n         four\n"
    );
    let hard_break = stable("@ol width=16\n- [ ] one two  \n    three four\\\n    five six\n");
    assert!(hard_break.contains(
        "- [ ] 1. one two  \n         three\n         four\\\n         five\n         six"
    ));
}

#[test]
fn wrapping_never_turns_inline_checkbox_text_into_a_new_item() {
    for checkbox in ["[ ]", "[x]", "[X]"] {
        let output = stable(&format!("@ol width=12\n- Hello {checkbox} more words\n"));
        assert!(output.contains(&format!("Hello {checkbox}")));
    }
}

#[test]
fn checkbox_anchors_and_references_keep_paths_independent_of_state() {
    let source = "See [](item:child).\n\n@ol start=3\n[ ] Parent\n  @item id=child\n  [x] 7. Child refers to [](item:child)\n";
    let output = stable(source);
    assert!(output.starts_with("See [3.a](item:child)."));
    assert!(output.contains("  @item id=child\n  - [x] a. Child refers to [3.a](item:child)"));
    let changed = stable(&output.replace("[x] a.", "[ ] a."));
    assert!(changed.starts_with("See [3.a](item:child)."));
    let moved = stable(&output.replace("  @item id=child\n  - [x] a.", "@item id=child\n- [x] a."));
    assert!(moved.starts_with("See [4](item:child)."));
}

#[test]
fn checkbox_reference_errors_keep_original_columns() {
    for source in [
        "@ol\n[ ] 1. 界 [](item:missing)\n",
        "@ol\n- [x] 界 [](item:missing)\n",
        "@ol\n- [x] 7. 界 [](item:missing)\n",
        "@ol\n- 7. 界 [](item:missing)\n",
        "@ol\n1. [X] 界 [](item:missing)\n",
        "@ol\n[ ] Parent\n  [x] a. 界 [](item:missing)\n",
        "@ol\n[ ] Parent\n        界 [](item:missing)\n",
    ] {
        let (line, text) = source
            .lines()
            .enumerate()
            .find(|(_, text)| text.contains("missing"))
            .unwrap();
        let offset = text.find("missing").unwrap();
        let diagnostic = format(source).unwrap_err().remove(0);
        assert_eq!(
            (diagnostic.line, diagnostic.column),
            (line + 1, text.split_at(offset).0.chars().count() + 1)
        );
        assert!(diagnostic.message.contains("unknown item reference"));
    }
}

#[test]
fn checkbox_body_text_and_non_ordered_lists_keep_their_meaning() {
    for source in [
        "[ ] 1. Plain prose\n",
        "- [ ] Bullet task\n- [X] Another\n",
        "7. [ ] Numbered task\n8. [x] Another\n",
        "```text\n@ol\n[ ] Task\n```\n",
        "    [ ] Task\n",
    ] {
        assert_eq!(stable(source), source);
    }
    assert_eq!(
        stable(
            "@ol\n- [ ] 12. Body text\n1. [x] 13. Body text\n- [-] Other state\n- [ ]No separator\n- `[ ]` Code\n"
        ),
        "@ol\n- [ ] 12. Body text\n- [x] 13. 13. Body text\n- 14. [-] Other state\n- 15. [ ]No separator\n- 16. `[ ]` Code\n"
    );
}

#[test]
fn removed_ul_directive_is_prose_and_bullet_lists_still_format() {
    assert_eq!(
        stable("@ul\n1. First\n2. Second\n"),
        "@ul\n\n1. First\n2. Second\n"
    );
    assert_eq!(
        stable("@ul width=off\n- First\n+ Second\n"),
        "@ul width=off\n\n- First\n- Second\n"
    );
    assert_eq!(
        stable("* [ ] First\n+ [x] Second\n"),
        "- [ ] First\n- [x] Second\n"
    );
}

#[test]
fn ids_are_unique_across_items_tables_and_separate_lists() {
    for source in [
        "@ol\n@item id=x\n- One\n@item id=x\n- Two\n",
        "@table id=x\n| h |\n\n@ol\n@item id=x\n- One\n",
        "@ol\n@item id=x\n- One\n\n@table id=x\n| h |\n",
        "@ol\n@item id=x\n- One\n\n@ol\n@item id=x\n- Two\n",
    ] {
        assert!(
            format(source)
                .unwrap_err()
                .first()
                .unwrap()
                .message
                .contains("duplicate ID 'x'")
        );
    }
}

#[test]
fn reference_errors_point_to_original_unicode_source_coordinates() {
    for (source, line, column) in [
        ("界 [](item:missing)\n", 1, 11),
        ("# 界 [](item:missing)\n", 1, 13),
        ("> 界 [](item:missing)\n", 1, 13),
        ("@ol\n- 界 [](item:missing)\n", 2, 13),
        ("| h |\n| 界 [](item:missing) |\n", 2, 13),
        ("h | ref\n--- | ---\n界 | [](item:missing)\n", 3, 13),
        ("@ol\n- root\n  - child\n    界 [](item:missing)\n", 4, 15),
    ] {
        let diagnostic = format(source).unwrap_err().remove(0);
        assert_eq!(
            (diagnostic.line, diagnostic.column),
            (line, column),
            "{source:?}: {diagnostic}"
        );
        assert!(diagnostic.message.contains("unknown item reference"));
    }
    assert!(
        format("[](item:1bad)\n")
            .unwrap_err()
            .first()
            .unwrap()
            .message
            .contains("invalid item reference")
    );
    assert!(
        format("@table id=x\n| h |\n\n[](item:x)\n")
            .unwrap_err()
            .first()
            .unwrap()
            .message
            .contains("targets a table")
    );
    assert!(
        format("> `unmatched\n>\n> [](item:missing)\n")
            .unwrap_err()
            .first()
            .unwrap()
            .message
            .contains("unknown item reference")
    );
}

#[test]
fn table_diagnostics_use_original_cells_before_delimiter_escaping() {
    for source in [
        "| Left | Right |\n| x│ | 界 [](item:missing) |\n",
        "Left | Right\n--- | ---\nmissing | 界 [](item:missing)\n",
        "┌────┬────┐\n│ Left │ Right │\n├────┼────┤\n│ x| │ 界 [](item:missing) │\n└────┴────┘\n",
    ] {
        let (line, text) = source
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("[](item:missing)"))
            .unwrap();
        let offset = text.rfind("missing").unwrap();
        let diagnostic = format(source).unwrap_err().remove(0);
        assert_eq!(
            (diagnostic.line, diagnostic.column),
            (line + 1, text.split_at(offset).0.chars().count() + 1)
        );
    }
}

#[test]
fn deleting_an_anchored_item_leaves_a_diagnostic_instead_of_a_stale_reference() {
    let output = stable("See [](item:target).\n\n@ol\n- Parent\n  @item id=target\n  - Target\n");
    let deleted = output.replace("  @item id=target\n  a. Target\n", "");
    let diagnostic = format(&deleted).unwrap_err().remove(0);
    assert!(
        diagnostic
            .message
            .contains("unknown item reference 'target'")
    );
    assert_eq!((diagnostic.line, diagnostic.column), (1, 16));
}

#[test]
fn references_change_table_measurements_and_generated_ids_avoid_item_ids() {
    let source = "@document width=10\n\n@ol\n@item id=table-1\n- Root\n  @item id=target\n  - Child\n\n@table\n| Reference |\n| [](item:target) |\n\nWrapping follows the reference table width.\n";
    let output = stable(source);
    assert!(output.contains("wrap=table:table-2"));
    assert!(output.contains("@table id=table-2\n"));
    assert!(output.contains("[1.a](item:target)"));
    let changed = stable(&output.replace("@ol\n", "@ol start=1234567890\n"));
    assert!(changed.contains("[1234567890.a](item:target)"));
    let width = |text: &str| {
        text.lines()
            .filter(|line| line.starts_with('|'))
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap()
    };
    assert!(width(&changed) > width(&output));
}
