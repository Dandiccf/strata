// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::tenxer_mode::Prompt;

/// Sorted: a.txt, alpha-report.txt, b.txt, beta.txt, c.txt, delta.txt,
/// gamma-report.md. "report" matches the second and last entries.
fn seed_report_names(fixture: &KeyboardFixture) {
    for name in [
        "alpha-report.txt",
        "beta.txt",
        "delta.txt",
        "gamma-report.md",
    ] {
        std::fs::write(fixture._directory.path().join(name), b"find").expect("fixture file");
    }
    let browser = fixture.view.browser();
    fixture.view.refresh();
    wait_loaded(&browser, 0);
    wait_until(|| entry_count(&browser) == 7);
}

/// Bound name labels (Columns and List) or inscriptions (Icons) that carry
/// find highlight attributes, including views not currently shown.
fn highlighted_names(widget: &gtk::Widget) -> Vec<String> {
    fn collect(widget: &gtk::Widget, names: &mut Vec<String>) {
        if let Some(label) = widget.downcast_ref::<gtk::Label>()
            && label.attributes().is_some()
        {
            names.push(label.text().to_string());
        }
        if let Some(label) = widget.downcast_ref::<gtk::Inscription>()
            && label.attributes().is_some()
        {
            names.push(label.text().unwrap_or_default().to_string());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            collect(&current, names);
            child = current.next_sibling();
        }
    }
    let mut names = Vec::new();
    collect(widget, &mut names);
    names.sort();
    names.dedup();
    names
}

fn type_and_submit(fixture: &KeyboardFixture, prompt: Key, text: &str) {
    assert!(fixture.press(prompt, ModifierType::empty()));
    assert!(fixture.shortcuts.prompt_has_focus());
    fixture.shortcuts.prompt().set_text(text);
    assert!(fixture.press(Key::Return, ModifierType::empty()));
    assert_eq!(fixture.shortcuts.open_prompt_kind(), None);
}

fn enable_tenxer(fixture: &KeyboardFixture) -> Rc<PreferenceManager> {
    let preferences = PreferenceManager::shared();
    fixture.shortcuts.bind_preferences(&preferences);
    preferences.set_tenxer_mode(true);
    pump(50);
    preferences
}

#[test]
fn tenxer_slash_covers_the_footer_with_a_focused_find_prompt() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::footer_prompt::tenxer_slash_covers_the_footer_with_a_focused_find_prompt",
        || {
            let fixture = KeyboardFixture::new();
            let preferences = PreferenceManager::shared();
            let browser = fixture.view.browser();
            fixture.shortcuts.bind_preferences(&preferences);

            focus_files(&fixture);
            fixture.press(Key::slash, ModifierType::empty());
            assert_eq!(
                fixture.shortcuts.open_prompt_kind(),
                None,
                "outside 10xer mode / keeps its type-to-search meaning"
            );

            preferences.set_tenxer_mode(true);
            pump(50);
            for (key, modifiers, kind, label) in [
                (Key::slash, ModifierType::empty(), Prompt::Find, "/"),
                (
                    Key::question,
                    ModifierType::SHIFT_MASK,
                    Prompt::FindBackward,
                    "?",
                ),
            ] {
                for mode in [BrowserMode::Columns, BrowserMode::List, BrowserMode::Icons] {
                    fixture.view.set_view_mode(mode);
                    focus_files(&fixture);
                    let focused = focused_name(&browser);
                    assert!(fixture.press(key, modifiers));
                    assert_eq!(fixture.shortcuts.open_prompt_kind(), Some(kind));
                    assert_eq!(
                        fixture.shortcuts.prompt_label().as_deref(),
                        Some(label),
                        "{mode:?}"
                    );
                    assert!(fixture.shortcuts.prompt_has_focus(), "{mode:?}");
                    assert!(fixture.shortcuts.prompt().text().is_empty());

                    fixture.press(Key::j, ModifierType::empty());
                    assert_eq!(
                        focused_name(&browser),
                        focused,
                        "{mode:?}: typing stays in the prompt"
                    );
                    assert!(fixture.press(Key::Escape, ModifierType::empty()));
                    assert_eq!(fixture.shortcuts.prompt_label(), None);
                    assert!(
                        fixture.view.item_view_has_focus(),
                        "{mode:?}: Escape returns focus to the listing"
                    );
                }
            }

            assert!(fixture.press(Key::slash, ModifierType::SHIFT_MASK));
            fixture.shortcuts.prompt().set_text("secret");
            preferences.set_tenxer_mode(false);
            pump(50);
            assert_eq!(fixture.shortcuts.open_prompt_kind(), None);
            assert_eq!(fixture.shortcuts.prompt_label(), None);
            assert!(fixture.shortcuts.prompt().text().is_empty());
            assert!(fixture.view.item_view_has_focus());
        },
    );
}

#[test]
fn tenxer_find_moves_between_matches_and_keeps_rows_visible() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::footer_prompt::tenxer_find_moves_between_matches_and_keeps_rows_visible",
        || {
            let fixture = KeyboardFixture::new();
            seed_report_names(&fixture);
            let preferences = enable_tenxer(&fixture);
            let browser = fixture.view.browser();
            let none = ModifierType::empty();
            let reports = vec!["alpha-report.txt".to_owned(), "gamma-report.md".to_owned()];

            for mode in [BrowserMode::Columns, BrowserMode::List, BrowserMode::Icons] {
                fixture.view.set_view_mode(mode);
                move_to_named(&fixture, &browser, "a.txt");

                type_and_submit(&fixture, Key::slash, "REPORT");
                assert_eq!(focused_name(&browser), "alpha-report.txt", "{mode:?}");
                assert!(fixture.view.item_view_has_focus(), "{mode:?}");
                wait_until(|| highlighted_names(&fixture.view.widget()) == reports);
                assert_eq!(entry_count(&browser), 7, "{mode:?}: find hides no rows");

                for (key, modifiers, expected) in [
                    (Key::n, none, "gamma-report.md"),
                    (Key::n, none, "alpha-report.txt"),
                    (Key::N, ModifierType::SHIFT_MASK, "gamma-report.md"),
                ] {
                    assert!(fixture.press(key, modifiers));
                    assert_eq!(focused_name(&browser), expected, "{mode:?} {key:?}");
                }

                type_and_submit(&fixture, Key::question, "report");
                assert_eq!(
                    focused_name(&browser),
                    "alpha-report.txt",
                    "{mode:?}: ? searches backward"
                );
                assert!(fixture.press(Key::n, none));
                assert_eq!(
                    focused_name(&browser),
                    "gamma-report.md",
                    "{mode:?}: n keeps the ? direction"
                );

                type_and_submit(&fixture, Key::slash, "zzz");
                assert_eq!(
                    focused_name(&browser),
                    "gamma-report.md",
                    "{mode:?}: a miss does not navigate"
                );
                assert_eq!(
                    fixture.shortcuts.feedback_text(),
                    "No matches for \u{201c}zzz\u{201d}"
                );
                assert_eq!(entry_count(&browser), 7);

                type_and_submit(&fixture, Key::slash, "report");
                type_and_submit(&fixture, Key::slash, "");
                assert_eq!(
                    focused_name(&browser),
                    "alpha-report.txt",
                    "{mode:?}: empty Enter does nothing"
                );
                wait_until(|| highlighted_names(&fixture.view.widget()) == reports);

                assert!(fixture.press(Key::Escape, none));
                wait_until(|| highlighted_names(&fixture.view.widget()).is_empty());
                assert_eq!(
                    focused_name(&browser),
                    "alpha-report.txt",
                    "{mode:?}: listing Escape dismisses highlights before the selection"
                );
                assert!(fixture.press(Key::n, none));
                assert_eq!(focused_name(&browser), "gamma-report.md");
                wait_until(|| highlighted_names(&fixture.view.widget()) == reports);

                assert!(fixture.press(Key::slash, none));
                assert!(fixture.press(Key::Escape, none));
                wait_until(|| highlighted_names(&fixture.view.widget()).is_empty());
                assert!(fixture.view.item_view_has_focus());
            }

            type_and_submit(&fixture, Key::slash, "report");
            wait_until(|| !highlighted_names(&fixture.view.widget()).is_empty());
            preferences.set_tenxer_mode(false);
            pump(50);
            wait_until(|| highlighted_names(&fixture.view.widget()).is_empty());
            preferences.set_tenxer_mode(true);
            pump(50);
            assert!(fixture.press(Key::n, none));
            assert_eq!(
                fixture.shortcuts.feedback_text(),
                "No previous find",
                "leaving the mode forgets the query"
            );
        },
    );
}

#[test]
fn tenxer_find_prompt_steers_the_listing_and_closes_on_focus_loss() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::footer_prompt::tenxer_find_prompt_steers_the_listing_and_closes_on_focus_loss",
        || {
            let fixture = KeyboardFixture::new();
            enable_tenxer(&fixture);
            let browser = fixture.view.browser();
            let none = ModifierType::empty();
            move_to_named(&fixture, &browser, "a.txt");

            assert!(fixture.press(Key::slash, none));
            fixture.shortcuts.prompt().set_text("b");
            for (key, expected) in [
                (Key::Down, "b.txt"),
                (Key::Down, "c.txt"),
                (Key::Up, "b.txt"),
            ] {
                assert!(fixture.press(key, none));
                pump(20);
                assert_eq!(focused_name(&browser), expected, "{key:?}");
                assert!(
                    fixture.shortcuts.prompt_has_focus(),
                    "{key:?} leaves the prompt focused"
                );
                assert_eq!(fixture.shortcuts.prompt().text(), "b");
            }

            let names = directory_names(fixture._directory.path());
            browser.focus_active();
            wait_until(|| fixture.shortcuts.open_prompt_kind().is_none());
            assert!(fixture.shortcuts.prompt().text().is_empty());
            assert_eq!(
                focused_name(&browser),
                "b.txt",
                "focus loss keeps the cursor"
            );
            assert!(fixture.view.item_view_has_focus());
            assert_eq!(directory_names(fixture._directory.path()), names);
            assert!(fixture.press(Key::n, none));
            assert_eq!(
                fixture.shortcuts.feedback_text(),
                "No previous find",
                "an abandoned prompt commits nothing"
            );
        },
    );
}

#[test]
fn leaving_tenxer_mode_clears_find_in_every_window() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::footer_prompt::leaving_tenxer_mode_clears_find_in_every_window",
        || {
            let first = KeyboardFixture::new();
            let second = KeyboardFixture::new();
            let preferences = enable_tenxer(&first);
            second.shortcuts.bind_preferences(&preferences);
            for fixture in [&first, &second] {
                focus_files(fixture);
                type_and_submit(fixture, Key::slash, "b");
                wait_until(|| highlighted_names(&fixture.view.widget()) == ["b.txt"]);
            }
            focus_files(&second);
            assert!(second.press(Key::question, ModifierType::SHIFT_MASK));
            second.shortcuts.prompt().set_text("draft");

            preferences.set_tenxer_mode(false);
            pump(50);
            for fixture in [&first, &second] {
                wait_until(|| highlighted_names(&fixture.view.widget()).is_empty());
                assert_eq!(fixture.shortcuts.open_prompt_kind(), None);
                assert!(fixture.shortcuts.prompt().text().is_empty());
                assert_eq!(fixture.view.find_query(), None);
            }
        },
    );
}
