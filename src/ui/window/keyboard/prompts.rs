// SPDX-License-Identifier: MIT

//! 10xer footer prompts. While a prompt has focus only its own keys act; every
//! other key edits the text and never reaches a browsing command.

use gtk::{
    gdk::{Key, ModifierType as Modifiers},
    glib::Propagation,
};

use super::{Dispatcher, KeyResult, command_modifiers};
use crate::{app::Browser, ui::tenxer_mode::Prompt};

fn plain(modifiers: Modifiers) -> bool {
    !command_modifiers(modifiers)
        .intersects(Modifiers::CONTROL_MASK | Modifiers::ALT_MASK | Modifiers::SUPER_MASK)
}

impl Dispatcher {
    /// **/**, **?**, **n**, and **N** from the listing. Shift is ignored because
    /// some layouts type **/** with it and **?** / **N** always need it.
    pub(super) fn tenxer_find_keys(&self, key: Key, modifiers: Modifiers) -> KeyResult {
        if !plain(modifiers) || !self.view.item_view_has_focus() {
            return None;
        }
        let prompt = match key {
            Key::slash | Key::KP_Divide => Prompt::Find,
            Key::question => Prompt::FindBackward,
            Key::n | Key::N => {
                self.repeat_find(key == Key::N);
                return Some(Propagation::Stop);
            }
            _ => return None,
        };
        self.shortcuts.open_prompt(prompt);
        Some(Propagation::Stop)
    }

    fn repeat_find(&self, reverse: bool) {
        match self.view.repeat_find(reverse, true) {
            None => self.shortcuts.show_feedback("No previous find"),
            Some(false) => self.report_miss(),
            Some(true) => {}
        }
    }

    fn report_miss(&self) {
        let query = self.view.find_query().unwrap_or_default();
        self.shortcuts
            .show_feedback(&format!("No matches for \u{201c}{query}\u{201d}"));
    }

    pub(super) fn prompt_key(
        &self,
        browser: &Browser,
        key: Key,
        modifiers: Modifiers,
    ) -> Propagation {
        let preferences = &self.type_to_search.preferences;
        if crate::ui::tenxer_mode::is_toggle_shortcut(key, modifiers) {
            preferences.set_tenxer_mode(!preferences.tenxer_mode());
            return Propagation::Stop;
        }
        if !plain(modifiers) {
            return Propagation::Proceed;
        }
        match key {
            Key::Escape => {
                self.view.dismiss_find_highlight();
                self.return_to_listing(browser);
            }
            Key::Return | Key::KP_Enter => self.submit_prompt(browser),
            Key::Up | Key::KP_Up => self.view.step_cursor_unfocused(-1),
            Key::Down | Key::KP_Down => self.view.step_cursor_unfocused(1),
            _ => return Propagation::Proceed,
        }
        Propagation::Stop
    }

    fn submit_prompt(&self, browser: &Browser) {
        let text = self.shortcuts.prompt_text();
        let found = match self.shortcuts.open_prompt_kind() {
            _ if text.is_empty() => true,
            Some(kind @ (Prompt::Find | Prompt::FindBackward)) => {
                self.view.find(&text, kind == Prompt::FindBackward, false)
            }
            None => true,
        };
        // The cursor already moved under the prompt; one focus move follows it.
        self.return_to_listing(browser);
        if !found {
            self.report_miss();
        }
    }

    fn return_to_listing(&self, browser: &Browser) {
        self.shortcuts.dismiss_prompt();
        browser.focus_active();
    }
}
