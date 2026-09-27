// SPDX-License-Identifier: MIT

use std::path::{Path, PathBuf};

use super::*;
use crate::ui::window::keyboard::chords::{GoTarget, go_target};

struct Places {
    home: PathBuf,
    downloads: PathBuf,
    pictures: PathBuf,
    config: PathBuf,
    first_pin: PathBuf,
    second_pin: PathBuf,
}

/// Downloads and Pictures exist; Documents and Videos are configured but
/// missing. Pins are stored beta, Downloads (hidden as a standard place), alpha.
fn disposable_places() -> Places {
    let home = PathBuf::from(std::env::var_os("HOME").expect("isolated HOME"));
    let config_home =
        PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").expect("isolated config home"));
    let places = Places {
        downloads: home.join("Downloads"),
        pictures: home.join("Pictures"),
        config: home.join(".config"),
        first_pin: home.join("pins/beta"),
        second_pin: home.join("pins/alpha"),
        home,
    };
    for directory in [
        &places.downloads,
        &places.pictures,
        &places.config,
        &places.first_pin,
        &places.second_pin,
    ] {
        std::fs::create_dir_all(directory).expect("place directory");
    }
    std::fs::create_dir_all(config_home.join("gtk-3.0")).expect("bookmark directory");
    std::fs::write(
        config_home.join("user-dirs.dirs"),
        "XDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\n\
         XDG_DOCUMENTS_DIR=\"$HOME/Documents\"\n\
         XDG_PICTURES_DIR=\"$HOME/Pictures\"\n\
         XDG_VIDEOS_DIR=\"$HOME/Videos\"\n",
    )
    .expect("user dirs");
    let bookmark =
        |path: &Path, name: &str| format!("{} {name}\n", gtk::gio::File::for_path(path).uri());
    std::fs::write(
        config_home.join("gtk-3.0/bookmarks"),
        [
            bookmark(&places.first_pin, "Beta"),
            bookmark(&places.downloads, "Downloads"),
            bookmark(&places.second_pin, "Alpha"),
        ]
        .concat(),
    )
    .expect("bookmarks");
    glib::reload_user_special_dirs_cache();
    places
}

fn visible_keycaps(fixture: &KeyboardFixture) -> Vec<String> {
    fn collect(widget: &gtk::Widget, keycaps: &mut Vec<String>) {
        if widget.has_css_class("sidebar-keycap")
            && let Some(label) = widget.downcast_ref::<gtk::Label>()
            && label.is_visible()
        {
            keycaps.push(label.text().to_string());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            collect(&current, keycaps);
            child = current.next_sibling();
        }
    }
    let mut keycaps = Vec::new();
    collect(&fixture.sidebar.widget, &mut keycaps);
    keycaps
}

fn chord(fixture: &KeyboardFixture, second: Key) {
    focus_files(fixture);
    fixture.shortcuts.dismiss_feedback();
    fixture.press(Key::g, ModifierType::empty());
    fixture.press(second, ModifierType::empty());
    assert_eq!(
        fixture.shortcuts.armed_chord(),
        None,
        "{second:?} ends the chord"
    );
}

#[test]
fn go_chord_resolves_uris_and_visible_pin_order() {
    let pins = [
        Location::local("/pins/beta"),
        Location::local("/pins/alpha"),
    ];
    for (key, location, validate) in [
        (Key::t, Location::uri("trash:///"), false),
        (Key::n, Location::uri("network:///"), true),
        (Key::r, Location::uri("recent:///"), false),
        (Key::_1, pins[0].clone(), true),
        (Key::KP_2, pins[1].clone(), true),
    ] {
        assert_eq!(
            go_target(key, &pins),
            Some(GoTarget::Place { location, validate }),
            "{key:?}"
        );
    }
    assert_eq!(
        go_target(Key::_9, &pins),
        Some(GoTarget::Missing("No pin 9".into()))
    );
    for key in [Key::z, Key::G, Key::q, Key::space, Key::_0] {
        assert_eq!(go_target(key, &pins), None, "{key:?} is not a place");
    }
}

#[test]
fn tenxer_go_chord_reaches_places_and_cancels_cleanly() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::place_chords::tenxer_go_chord_reaches_places_and_cancels_cleanly",
        || {
            let places = disposable_places();
            let fixture = KeyboardFixture::new();
            let preferences = PreferenceManager::shared();
            preferences.set_tenxer_mode(true);
            preferences.set_sidebar_show_downloads(true);
            preferences.set_sidebar_show_documents(true);
            preferences.set_sidebar_show_pictures(true);
            preferences.set_sidebar_show_videos(true);
            fixture.shortcuts.bind_preferences(&preferences);
            let browser = fixture.view.browser();
            let origin = browser.active_location();
            assert!(visible_keycaps(&fixture).is_empty());

            focus_files(&fixture);
            fixture.press(Key::g, ModifierType::empty());
            assert_eq!(fixture.shortcuts.chord().text(), "g-");
            assert!(fixture.shortcuts.chord().is_visible());
            assert!(
                fixture
                    .shortcuts
                    .chord_hint()
                    .is_some_and(|hint| hint.contains("h home") && hint.contains("1–9 pins"))
            );
            let keycaps = visible_keycaps(&fixture);
            for key in ["h", "d", "k", "p", "v", "1", "2"] {
                assert!(
                    keycaps.contains(&key.to_owned()),
                    "{key} keycap in {keycaps:?}"
                );
            }
            assert!(
                !keycaps.contains(&"3".to_owned()),
                "hidden pins get no keycap"
            );
            fixture.press(Key::Escape, ModifierType::empty());
            assert!(!fixture.shortcuts.chord().is_visible());
            assert_eq!(fixture.shortcuts.chord_hint(), None);
            assert!(visible_keycaps(&fixture).is_empty());
            fixture.press(Key::d, ModifierType::empty());
            pump(50);
            assert_eq!(browser.active_location(), origin, "Esc left no pending d");

            for (key, feedback) in [
                (Key::z, "Unknown chord"),
                (Key::q, "Unknown chord"),
                (Key::k, "No Documents folder"),
                (Key::v, "No Videos folder"),
                (Key::_3, "No pin 3"),
            ] {
                chord(&fixture, key);
                assert_eq!(fixture.shortcuts.feedback_text(), feedback, "g {key:?}");
                pump(20);
                assert_eq!(browser.active_location(), origin, "g {key:?} stays");
                assert!(preferences.tenxer_mode(), "g {key:?} is not q");
            }

            for mode in [BrowserMode::Columns, BrowserMode::Icons] {
                fixture.view.set_view_mode(mode);
                focus_files(&fixture);
                fixture.press(Key::G, ModifierType::SHIFT_MASK);
                wait_until(|| focused_index(&browser) == 2);
                chord(&fixture, Key::g);
                wait_until(|| focused_index(&browser) == 0);
                assert_eq!(browser.active_location(), origin, "{mode:?} g g");
            }
            fixture.view.set_view_mode(BrowserMode::Columns);

            for (key, destination) in [
                (Key::h, &places.home),
                (Key::d, &places.downloads),
                (Key::p, &places.pictures),
                (Key::c, &places.config),
                (Key::_1, &places.first_pin),
                (Key::_2, &places.second_pin),
            ] {
                chord(&fixture, key);
                wait_until(|| browser.active_location() == Some(Location::local(destination)));
                wait_loaded(&browser, 0);
            }
            chord(&fixture, Key::t);
            assert_eq!(browser.active_location(), Some(Location::uri("trash:///")));

            browser.navigate(Location::local(&places.home));
            wait_loaded(&browser, 0);
            focus_files(&fixture);
            fixture.press(Key::g, ModifierType::empty());
            preferences.set_tenxer_mode(false);
            pump(20);
            assert_eq!(fixture.shortcuts.armed_chord(), None);
            assert!(!fixture.shortcuts.chord().is_visible());
            assert!(visible_keycaps(&fixture).is_empty());
            preferences.set_tenxer_mode(true);
            focus_files(&fixture);
            fixture.press(Key::d, ModifierType::empty());
            pump(50);
            assert_eq!(
                browser.active_location(),
                Some(Location::local(&places.home)),
                "mode exit left no pending g"
            );

            fixture.press(Key::g, ModifierType::empty());
            fixture.press(Key::comma, ModifierType::CONTROL_MASK);
            assert_eq!(
                fixture.shortcuts.armed_chord(),
                None,
                "Settings entry cancels"
            );

            fixture.press(Key::g, ModifierType::empty());
            assert_eq!(
                fixture.shortcuts.armed_chord(),
                Some(crate::ui::tenxer_mode::Chord::Go)
            );
            fixture.window.destroy();
            assert_eq!(
                fixture.shortcuts.armed_chord(),
                None,
                "window destruction cancels"
            );
        },
    );
}
