// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use gtk::{gdk, glib, prelude::*};

use super::{
    browser::FilterStatus,
    browser_modes::BrowserMode,
    tenxer_mode::{Chord, Prompt},
};

type Shortcut = (&'static str, &'static str);
type ChordListener = Box<dyn Fn(Option<Chord>)>;
/// `None` while the view is busy rebuilding; the footer then retries on idle.
type FilterSource = Rc<RefCell<Option<Rc<dyn Fn() -> Option<Option<FilterStatus>>>>>>;

#[derive(Clone)]
pub(super) struct ShortcutFooter {
    root: gtk::Stack,
    status: gtk::Box,
    paste: gtk::Label,
    count: gtk::Label,
    show_hints: Rc<Cell<bool>>,
    pending_popup: Rc<Cell<bool>>,
    more: gtk::MenuButton,
    popover: gtk::Popover,
    reference: gtk::Box,
    scroll: gtk::ScrolledWindow,
    focus_before: Rc<RefCell<Option<glib::WeakRef<gtk::Widget>>>>,
    status_widgets: Rc<RefCell<Vec<gtk::Widget>>>,
    tag: gtk::Label,
    experimental: gtk::Label,
    feedback: gtk::Label,
    feedback_epoch: Rc<Cell<u64>>,
    prompt: PromptBar,
    chords: ChordIndicator,
    visual: gtk::Label,
    filter: gtk::Label,
    current: CurrentHit,
    filter_source: FilterSource,
    filter_retry: Rc<Cell<bool>>,
    observed: Rc<RefCell<std::rc::Weak<crate::app::Browser>>>,
    view_mode: Rc<Cell<BrowserMode>>,
}

/// The search hit under the cursor, as its path below the searched folder.
/// The folder part gives way to an ellipsis before the name does.
#[derive(Clone)]
struct CurrentHit {
    root: gtk::Box,
    folder: gtk::Label,
    name: gtk::Label,
}

impl CurrentHit {
    /// Names longer than this may be ellipsized in a narrow footer.
    const NAME_MIN_CHARS: usize = 32;

    fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.set_visible(false);
        let folder = gtk::Label::new(None);
        folder.set_ellipsize(gtk::pango::EllipsizeMode::End);
        folder.set_max_width_chars(60);
        let name = gtk::Label::new(None);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        for label in [&folder, &name] {
            label.add_css_class("shortcut-footer-chord-hint");
            root.append(label);
        }
        Self { root, folder, name }
    }

    fn set(&self, path: Option<&std::path::Path>) {
        let Some(path) = path.filter(|path| path.file_name().is_some()) else {
            self.root.set_visible(false);
            return;
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let folder = path
            .parent()
            .map(|folder| folder.to_string_lossy())
            .unwrap_or_default();
        self.folder.set_text(&folder);
        self.folder.set_visible(!folder.is_empty());
        let name = if folder.is_empty() {
            name.into_owned()
        } else {
            format!("{}{name}", std::path::MAIN_SEPARATOR)
        };
        self.name.set_text(&name);
        let chars = name.chars().count();
        self.name
            .set_width_chars(chars.min(Self::NAME_MIN_CHARS) as i32);
        let full = path.to_string_lossy();
        self.root.set_tooltip_text(Some(&full));
        super::accessibility::set_label(&self.root, &full);
        self.root.set_visible(true);
    }
}

/// The armed chord and its footer mark. The chord is armed exactly while the
/// mark is showing.
#[derive(Clone)]
struct ChordIndicator {
    armed: Rc<Cell<Option<Chord>>>,
    mark: glib::WeakRef<gtk::Label>,
    hint: glib::WeakRef<gtk::Label>,
    listeners: Rc<RefCell<Vec<ChordListener>>>,
}

impl ChordIndicator {
    fn set(&self, chord: Option<Chord>) {
        if let Some(mark) = self.mark.upgrade() {
            mark.set_text(chord.map_or("", Chord::mark));
            mark.set_visible(chord.is_some());
        }
        if let Some(hint) = self.hint.upgrade() {
            hint.set_text(chord.map_or("", Chord::hint));
            hint.set_visible(chord.is_some());
        }
        if self.armed.replace(chord) != chord {
            for listener in self.listeners.borrow().iter() {
                listener(chord);
            }
        }
    }
}

/// The open footer prompt. Its bar covers the whole footer so typing never
/// shares the row with status marks.
#[derive(Clone)]
struct PromptBar {
    bar: gtk::Box,
    label: gtk::Label,
    entry: gtk::Entry,
    kind: Rc<Cell<Option<Prompt>>>,
}

#[derive(Clone)]
struct WeakPromptBar {
    bar: glib::WeakRef<gtk::Box>,
    label: glib::WeakRef<gtk::Label>,
    entry: glib::WeakRef<gtk::Entry>,
    kind: Rc<Cell<Option<Prompt>>>,
}

impl PromptBar {
    fn new() -> Self {
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.add_css_class("shortcut-footer-prompt-bar");
        bar.set_visible(false);
        let label = gtk::Label::new(None);
        label.add_css_class("shortcut-footer-prompt-label");
        let entry = gtk::Entry::new();
        entry.add_css_class("form-control");
        entry.add_css_class("shortcut-footer-prompt");
        entry.set_hexpand(true);
        bar.append(&label);
        bar.append(&entry);
        Self {
            bar,
            label,
            entry,
            kind: Rc::new(Cell::new(None)),
        }
    }

    fn downgrade(&self) -> WeakPromptBar {
        WeakPromptBar {
            bar: self.bar.downgrade(),
            label: self.label.downgrade(),
            entry: self.entry.downgrade(),
            kind: self.kind.clone(),
        }
    }

    fn has_focus(&self) -> bool {
        self.bar.is_visible()
            && self
                .bar
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|focus| {
                    focus == *self.entry.upcast_ref::<gtk::Widget>()
                        || focus.is_ancestor(&self.entry)
                })
    }

    fn open(&self, stack: &gtk::Stack, kind: Prompt, text: &str) -> bool {
        // Replacing an open prompt must not hand its text to the new kind.
        self.kind.set(None);
        self.entry.set_text("");
        self.kind.set(Some(kind));
        self.label.set_text(kind.label());
        super::accessibility::set_label(&self.entry, kind.name());
        self.entry.set_text(text);
        self.bar.set_visible(true);
        stack.set_visible_child(&self.bar);
        // A pre-filled query stays unselected so typing extends it.
        let focused = self.entry.grab_focus_without_selecting();
        self.entry.set_position(-1);
        focused
    }

    /// Clears the typed text so nothing lingers. Callers own where focus goes.
    fn close(&self) {
        self.kind.set(None);
        self.entry.set_text("");
        self.bar.set_visible(false);
    }
}

impl WeakPromptBar {
    fn upgrade(&self) -> Option<PromptBar> {
        Some(PromptBar {
            bar: self.bar.upgrade()?,
            label: self.label.upgrade()?,
            entry: self.entry.upgrade()?,
            kind: self.kind.clone(),
        })
    }
}

impl ShortcutFooter {
    pub fn new(mode: BrowserMode) -> Self {
        let root = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .build();
        root.add_css_class("shortcut-footer");
        let status = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let count = gtk::Label::new(None);
        count.add_css_class("shortcut-footer-count");
        count.set_visible(false);
        let paste = gtk::Label::new(Some("Files on clipboard"));
        paste.add_css_class("shortcut-footer-paste");
        paste.set_tooltip_text(Some("Press Ctrl+V to paste into a supported directory."));
        paste.set_visible(false);
        let tag = gtk::Label::new(Some(crate::ui::tenxer_mode::TAG_TEXT));
        tag.add_css_class("tenxer-tag");
        tag.set_tooltip_text(Some(crate::ui::tenxer_mode::TAG_NAME));
        super::accessibility::set_label(&tag, crate::ui::tenxer_mode::TAG_NAME);
        tag.set_visible(false);
        let chord = gtk::Label::new(None);
        chord.add_css_class("shortcut-footer-chord");
        chord.set_visible(false);
        let chord_hint = gtk::Label::new(None);
        chord_hint.add_css_class("shortcut-footer-chord-hint");
        chord_hint.set_ellipsize(gtk::pango::EllipsizeMode::End);
        chord_hint.set_visible(false);
        let chords = ChordIndicator {
            armed: Rc::new(Cell::new(None)),
            mark: chord.downgrade(),
            hint: chord_hint.downgrade(),
            listeners: Rc::new(RefCell::new(Vec::new())),
        };
        let visual = gtk::Label::new(None);
        visual.add_css_class("shortcut-footer-chord");
        visual.set_visible(false);
        let filter = gtk::Label::new(None);
        filter.add_css_class("shortcut-footer-chord");
        filter.set_ellipsize(gtk::pango::EllipsizeMode::End);
        filter.set_max_width_chars(40);
        filter.set_visible(false);
        let current = CurrentHit::new();
        let prompt = PromptBar::new();
        let feedback = gtk::Label::new(None);
        feedback.add_css_class("shortcut-footer-feedback");
        feedback.set_visible(false);
        status.append(&paste);
        status.append(&filter);
        status.append(&visual);
        status.append(&chord);
        status.append(&chord_hint);
        // Transient marks grow leftward so the pill stays put.
        status.append(&tag);
        status.append(&feedback);
        status.append(&count);
        root.add_child(&status);
        root.add_child(&prompt.bar);
        let show_hints = Rc::new(Cell::new(true));
        let pending_popup = Rc::new(Cell::new(false));

        let more = gtk::MenuButton::new();
        more.set_child(Some(&gtk::Label::new(Some("F1  Shortcuts"))));
        more.add_css_class("shortcut-footer-button");
        more.set_tooltip_text(Some("Show all file-view shortcuts (F1)"));
        status.prepend(&more);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        // The hit path sits left, away from the marks and count on the right.
        status.insert_child_after(&current.root, Some(&more));
        status.insert_child_after(&spacer, Some(&current.root));
        let popover = gtk::Popover::builder()
            .position(gtk::PositionType::Top)
            .halign(gtk::Align::Start)
            .has_arrow(false)
            .build();
        popover.add_css_class("shortcut-popover");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let title = gtk::Label::builder()
            .label("Keyboard shortcuts")
            .xalign(0.0)
            .hexpand(true)
            .build();
        title.add_css_class("shortcut-reference-title");
        content.append(&title);
        let dismiss_note = gtk::Label::builder()
            .label("Press F1 again to close.")
            .xalign(0.0)
            .wrap(true)
            .build();
        dismiss_note.add_css_class("shortcut-reference-note");
        content.append(&dismiss_note);
        let note = gtk::Label::builder()
            .label("Media controls use Ctrl+Alt. Plain keys keep browsing; text fields and dialogs keep native controls.")
            .xalign(0.0).wrap(true).build();
        note.add_css_class("shortcut-reference-note");
        content.append(&note);
        let experimental = gtk::Label::new(None);
        experimental.add_css_class("shortcut-reference-note");
        experimental.add_css_class("tenxer-experimental");
        experimental.set_xalign(0.0);
        experimental.set_wrap(true);
        experimental.set_visible(false);
        content.append(&experimental);
        let reference = gtk::Box::new(gtk::Orientation::Vertical, 16);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&reference)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .max_content_height(440)
            .width_request(420)
            .focusable(true)
            .build();
        scroll.add_css_class("shortcut-reference-scroll");
        content.append(&scroll);
        popover.set_child(Some(&content));
        let weak_scroll = scroll.downgrade();
        let weak_popover = popover.downgrade();
        popover.connect_show(move |popover| {
            let Some(scroll) = weak_scroll.upgrade() else {
                return;
            };
            if let Some(window) = popover.root().and_downcast::<gtk::Window>() {
                scroll.vadjustment().set_value(scroll.vadjustment().lower());
                scroll.set_max_content_height((window.height() - 150).clamp(100, 440));
                scroll.set_width_request((window.width() - 60).clamp(260, 420));
            }
            let scroll = scroll.downgrade();
            let popover = weak_popover.clone();
            // Show runs before the popover can take focus; grab it once mapped.
            glib::idle_add_local_once(move || {
                if popover
                    .upgrade()
                    .is_some_and(|popover| popover.is_visible())
                    && let Some(scroll) = scroll.upgrade()
                {
                    scroll.grab_focus();
                }
            });
        });
        more.set_popover(Some(&popover));
        let focus_before: Rc<RefCell<Option<glib::WeakRef<gtk::Widget>>>> =
            Rc::new(RefCell::new(None));
        let restored_focus = focus_before.clone();
        let weak_more = more.downgrade();
        let closed_hints = show_hints.clone();
        let closed_pending = pending_popup.clone();
        let weak_popover = popover.downgrade();
        popover.connect_closed(move |_| {
            let restored_focus = restored_focus.clone();
            let closed_pending = closed_pending.clone();
            let weak_more = weak_more.clone();
            let closed_hints = closed_hints.clone();
            let weak_popover = weak_popover.clone();
            // MenuButton restores its own focus after ::closed; wait without overriding a newer focus move.
            glib::idle_add_local_once(move || {
                if closed_pending.get()
                    || weak_popover
                        .upgrade()
                        .is_some_and(|popover| popover.is_visible())
                {
                    return;
                }
                let previous = restored_focus.borrow_mut().take();
                let Some(more) = weak_more.upgrade() else {
                    return;
                };
                let still_on_button =
                    more.root()
                        .and_then(|root| root.focus())
                        .is_some_and(|focused| {
                            focused == *more.upcast_ref::<gtk::Widget>()
                                || focused.is_ancestor(&more)
                        });
                if still_on_button
                    && let Some(previous) = previous.and_then(|previous| previous.upgrade())
                    && previous.is_mapped()
                {
                    previous.grab_focus();
                }
                more.set_visible(closed_hints.get());
            });
        });
        let status_widgets: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(vec![
            paste.clone().upcast::<gtk::Widget>(),
            count.clone().upcast(),
            more.clone().upcast(),
            tag.clone().upcast(),
            chord.clone().upcast(),
            chord_hint.clone().upcast(),
            visual.clone().upcast(),
            filter.clone().upcast(),
            current.root.clone().upcast(),
            prompt.bar.clone().upcast(),
            feedback.clone().upcast(),
        ]));
        for widget in status_widgets.borrow().iter() {
            watch_status_widget(widget, &status_widgets, &root);
        }
        let footer = Self {
            root,
            status,
            paste,
            count,
            show_hints,
            pending_popup,
            more,
            popover,
            reference,
            scroll,
            focus_before,
            status_widgets,
            tag,
            experimental,
            feedback,
            feedback_epoch: Rc::new(Cell::new(0)),
            prompt,
            chords,
            visual,
            filter,
            current,
            filter_source: Rc::new(RefCell::new(None)),
            filter_retry: Rc::new(Cell::new(false)),
            observed: Rc::new(RefCell::new(std::rc::Weak::new())),
            view_mode: Rc::new(Cell::new(mode)),
        };
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let shortcuts = footer.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            shortcuts
                .handle_key(key, modifiers)
                .unwrap_or(glib::Propagation::Proceed)
        });
        footer.popover.add_controller(keys);
        footer.close_prompt_on_focus_loss();
        footer.set_mode(mode);
        footer
    }

    /// Focus moving anywhere but the shortcut reference (a clicked row, another
    /// pane) discards the prompt, so its text never reaches a browsing command.
    fn close_prompt_on_focus_loss(&self) {
        let focus = gtk::EventControllerFocus::new();
        let prompt = self.prompt.downgrade();
        let popover = self.popover.downgrade();
        let pending_popup = self.pending_popup.clone();
        focus.connect_leave(move |_| {
            let prompt = prompt.clone();
            let popover = popover.clone();
            let pending_popup = pending_popup.clone();
            // Focus has not settled on its new owner while ::leave runs.
            glib::idle_add_local_once(move || {
                let reference_open = pending_popup.get()
                    || popover
                        .upgrade()
                        .is_some_and(|popover| popover.is_visible());
                if let Some(prompt) = prompt.upgrade()
                    && prompt.bar.is_visible()
                    && !prompt.has_focus()
                    && !reference_open
                {
                    prompt.close();
                }
            });
        });
        self.prompt.entry.add_controller(focus);
    }

    pub fn widget(&self) -> &gtk::Stack {
        &self.root
    }

    pub fn set_activity(&self, widget: &impl IsA<gtk::Widget>) {
        let widget = widget.as_ref().clone();
        self.status.insert_child_after(&widget, Some(&self.count));
        self.status_widgets.borrow_mut().push(widget.clone());
        watch_status_widget(&widget, &self.status_widgets, &self.root);
    }

    pub fn bind_preferences(&self, manager: &super::preferences::PreferenceManager) {
        let tag = self.tag.downgrade();
        let experimental = self.experimental.downgrade();
        let reference = self.reference.downgrade();
        let popover = self.popover.downgrade();
        let view_mode = self.view_mode.clone();
        let feedback = self.feedback.downgrade();
        let prompt = self.prompt.downgrade();
        let chords = self.chords.clone();
        let primed = Rc::new(Cell::new(false));
        manager.bind_preference(
            &self.root,
            super::preferences::PreferenceManager::tenxer_mode,
            move |_, enabled| {
                let Some(tag) = tag.upgrade() else {
                    return;
                };
                let Some(experimental) = experimental.upgrade() else {
                    return;
                };
                let Some(reference) = reference.upgrade() else {
                    return;
                };
                let starting = !primed.replace(true);
                apply_experimental_label(&tag, &experimental, enabled);
                if let Some(popover) = popover.upgrade() {
                    if enabled {
                        popover.add_css_class("tenxer-active");
                    } else {
                        popover.remove_css_class("tenxer-active");
                    }
                }
                if !starting
                    && !enabled
                    && let Some(feedback) = feedback.upgrade()
                {
                    feedback.set_text("");
                    feedback.set_visible(false);
                    if let Some(prompt) = prompt.upgrade() {
                        prompt.close();
                    }
                    chords.set(None);
                }
                rebuild_reference(&reference, view_mode.get());
            },
        );
        let show_hints = self.show_hints.clone();
        let pending = self.pending_popup.clone();
        let weak_popover = self.popover.downgrade();
        let more = self.more.downgrade();
        manager.on_keybinding_hints_changed(&self.root, move |_, enabled| {
            show_hints.set(enabled);
            if !enabled {
                pending.set(false);
            }
            if let Some(more) = more.upgrade() {
                more.set_visible(
                    enabled
                        || weak_popover
                            .upgrade()
                            .is_some_and(|popover| popover.is_visible()),
                );
            }
        });
    }

    pub fn connect_clipboard(&self, clipboard: &gdk::Clipboard) -> glib::SignalHandlerId {
        let label = self.paste.downgrade();
        let generation = Rc::new(Cell::new(0));
        refresh_paste_availability(clipboard, &label, &generation);
        clipboard.connect_changed(move |clipboard| {
            refresh_paste_availability(clipboard, &label, &generation);
        })
    }

    pub fn observe_browser(&self, browser: &Rc<crate::app::Browser>) {
        self.observed.replace(Rc::downgrade(browser));
        self.refresh_filter();
        let weak_browser = Rc::downgrade(browser);
        let footer = self.clone();
        browser.observe(move |event| {
            if matches!(
                event,
                crate::app::BrowserEvent::NavigationStarting
                    | crate::app::BrowserEvent::SelectionSetChanged { .. }
            ) {
                footer.clear_feedback();
            }
            if weak_browser.upgrade().is_some() {
                footer.refresh_filter();
            }
        });
    }

    /// Refreshes once on idle, after focus settles on the cursor row.
    pub(in crate::ui) fn schedule_filter_refresh(&self) {
        if self.filter_retry.replace(true) {
            return;
        }
        let footer = self.clone();
        glib::idle_add_local_once(move || {
            footer.filter_retry.set(false);
            footer.refresh_filter();
        });
    }

    /// Reports `source`'s filter in place of the directory count while one is
    /// active. Call [`Self::refresh_filter`] when it changes.
    pub(in crate::ui) fn observe_filter(
        &self,
        source: impl Fn() -> Option<Option<FilterStatus>> + 'static,
    ) {
        self.filter_source.replace(Some(Rc::new(source)));
        self.refresh_filter();
    }

    /// Updates the `filter:` or `search:` mark, the current search hit, and
    /// the count from the observed filter.
    pub(in crate::ui) fn refresh_filter(&self) {
        let source = self.filter_source.borrow().clone();
        let status = match source {
            Some(source) => match source() {
                Some(status) => status,
                None => {
                    self.schedule_filter_refresh();
                    return;
                }
            },
            None => None,
        };
        match status.as_ref() {
            Some(status) => {
                let label = if status.search { "search" } else { "filter" };
                self.filter.set_text(&format!("{label}: {}", status.query));
                self.filter.set_tooltip_text(Some(&status.query));
                self.filter.set_visible(true);
            }
            None => self.filter.set_visible(false),
        }
        self.current
            .set(status.as_ref().and_then(|status| status.current.as_deref()));
        let browser = self.observed.borrow().upgrade();
        if let Some(browser) = browser {
            update_item_count(&self.count, &browser, status.as_ref());
            // A range over results is theirs; the hidden directory's never shows.
            let visual = match status.as_ref() {
                Some(status) if status.visual.is_some() => status.visual,
                _ => browser.visual_kind(),
            };
            update_visual_mode(&self.visual, visual);
        }
    }

    #[cfg(test)]
    pub(in crate::ui) fn tag_visible(&self) -> bool {
        self.tag.is_visible()
    }

    pub fn set_mode(&self, mode: BrowserMode) {
        self.view_mode.set(mode);
        rebuild_reference(&self.reference, mode);
    }

    pub(in crate::ui) fn show_feedback(&self, text: &str) {
        let epoch = self.feedback_epoch.get().wrapping_add(1);
        self.feedback_epoch.set(epoch);
        self.feedback.set_text(text);
        let visible = !text.is_empty();
        self.feedback.set_visible(visible);
        if !visible {
            return;
        }
        let epochs = self.feedback_epoch.clone();
        let feedback = self.feedback.downgrade();
        glib::timeout_add_local_once(FEEDBACK_FLASH, move || {
            if epochs.get() != epoch {
                return;
            }
            if let Some(feedback) = feedback.upgrade() {
                feedback.set_text("");
                feedback.set_visible(false);
            }
        });
    }

    pub(in crate::ui) fn clear_feedback(&self) {
        self.feedback_epoch
            .set(self.feedback_epoch.get().wrapping_add(1));
        self.feedback.set_text("");
        self.feedback.set_visible(false);
    }

    #[cfg(test)]
    pub(in crate::ui) fn feedback_text(&self) -> String {
        self.feedback.text().to_string()
    }

    #[cfg(test)]
    pub(in crate::ui) fn dismiss_feedback(&self) {
        self.clear_feedback();
    }

    /// Covers the footer with `kind`'s prompt and focuses its entry.
    pub(in crate::ui) fn open_prompt(&self, kind: Prompt) -> bool {
        self.open_prompt_with(kind, "")
    }

    /// Opens `kind`'s prompt pre-filled with `text`.
    pub(in crate::ui) fn open_prompt_with(&self, kind: Prompt, text: &str) -> bool {
        // An armed chord must stay visible, and the prompt covers its mark.
        self.chords.set(None);
        self.prompt.open(&self.root, kind, text)
    }

    /// Runs `listener` as the open prompt's text changes, not when a prompt
    /// opens empty or closes.
    pub(in crate::ui) fn connect_prompt_changed(
        &self,
        listener: impl Fn(Prompt, String) + 'static,
    ) {
        let kind = self.prompt.kind.clone();
        self.prompt.entry.connect_changed(move |entry| {
            if let Some(kind) = kind.get() {
                listener(kind, entry.text().to_string());
            }
        });
    }

    #[cfg(test)]
    pub(in crate::ui) fn filter_mark(&self) -> Option<String> {
        self.filter
            .is_visible()
            .then(|| self.filter.text().to_string())
    }

    /// The current search hit's path as shown, and in full.
    #[cfg(test)]
    pub(in crate::ui) fn current_hit(&self) -> Option<(String, String)> {
        let current = &self.current;
        current.root.is_visible().then(|| {
            let folder = if current.folder.is_visible() {
                current.folder.text().to_string()
            } else {
                String::new()
            };
            (
                format!("{folder}{}", current.name.text()),
                current.root.tooltip_text().unwrap_or_default().to_string(),
            )
        })
    }

    #[cfg(test)]
    pub(in crate::ui) fn count_text(&self) -> (String, String) {
        (
            self.count.text().to_string(),
            self.count.tooltip_text().unwrap_or_default().to_string(),
        )
    }

    #[cfg(test)]
    pub(in crate::ui) fn prompt(&self) -> &gtk::Entry {
        &self.prompt.entry
    }

    pub(in crate::ui) fn open_prompt_kind(&self) -> Option<Prompt> {
        self.prompt.kind.get()
    }

    pub(in crate::ui) fn prompt_text(&self) -> String {
        self.prompt.entry.text().to_string()
    }

    #[cfg(test)]
    pub(in crate::ui) fn prompt_label(&self) -> Option<String> {
        (self.root.visible_child().as_ref() == Some(self.prompt.bar.upcast_ref()))
            .then(|| self.prompt.label.text().to_string())
    }

    /// Closes the prompt without moving focus; the caller returns it to the
    /// listing.
    pub(in crate::ui) fn dismiss_prompt(&self) {
        self.prompt.close();
    }

    pub(in crate::ui) fn arm_chord(&self, chord: Chord) {
        self.chords.set(Some(chord));
    }

    pub(in crate::ui) fn armed_chord(&self) -> Option<Chord> {
        self.chords.armed.get()
    }

    pub(in crate::ui) fn cancel_chord(&self) {
        self.chords.set(None);
    }

    pub(in crate::ui) fn connect_chord_changed(&self, listener: impl Fn(Option<Chord>) + 'static) {
        self.chords.listeners.borrow_mut().push(Box::new(listener));
    }

    #[cfg(test)]
    pub(in crate::ui) fn chord(&self) -> gtk::Label {
        self.chords.mark.upgrade().expect("chord mark")
    }

    #[cfg(test)]
    pub(in crate::ui) fn chord_hint(&self) -> Option<String> {
        let hint = self.chords.hint.upgrade()?;
        hint.is_visible().then(|| hint.text().to_string())
    }

    pub(in crate::ui) fn prompt_has_focus(&self) -> bool {
        self.prompt.has_focus()
    }

    pub fn handle_key(
        &self,
        key: gdk::Key,
        modifiers: gdk::ModifierType,
    ) -> Option<glib::Propagation> {
        let command_modifiers = modifiers.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        );
        let reference_open = self.popover.is_visible() || self.pending_popup.get();
        let f1 = key == gdk::Key::F1
            && !command_modifiers
            && !modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        let tilde = self.tilde_toggles(key, modifiers, reference_open);
        if self.prompt_has_focus() && !f1 && !tilde && !reference_open {
            return None;
        }
        if f1 || tilde {
            if self.popover.is_visible() || self.pending_popup.replace(false) {
                if self.popover.is_visible() {
                    self.more.popdown();
                } else {
                    self.focus_before.take();
                    self.more.set_visible(self.show_hints.get());
                }
            } else {
                if self.focus_before.borrow().is_none() {
                    self.focus_before.replace(
                        self.root
                            .root()
                            .and_then(|root| root.focus())
                            .map(|widget| widget.downgrade()),
                    );
                }
                if self.more.is_mapped() && self.more.width() > 0 {
                    self.more.popup();
                } else {
                    self.pending_popup.set(true);
                    self.more.set_visible(true);
                    let pending = self.pending_popup.clone();
                    let weak_more = self.more.downgrade();
                    // A hidden shortcut button needs an allocation before positioning the popover.
                    self.root.add_tick_callback(move |_, _| {
                        let Some(more) = weak_more.upgrade() else {
                            return glib::ControlFlow::Break;
                        };
                        if !pending.get() {
                            return glib::ControlFlow::Break;
                        }
                        if !more.is_mapped() || more.width() == 0 {
                            return glib::ControlFlow::Continue;
                        }
                        pending.set(false);
                        more.popup();
                        glib::ControlFlow::Break
                    });
                }
            }
            return Some(glib::Propagation::Stop);
        }
        if self.pending_popup.get() {
            if key == gdk::Key::Escape {
                self.pending_popup.set(false);
                self.focus_before.take();
                self.more.set_visible(self.show_hints.get());
            }
            return Some(glib::Propagation::Stop);
        }
        if !self.popover.is_visible() {
            return None;
        }
        if key == gdk::Key::Escape {
            self.more.popdown();
            return Some(glib::Propagation::Stop);
        }
        if !command_modifiers && self.scroll_reference(key) {
            return Some(glib::Propagation::Stop);
        }
        // The reference is read-only: never let a shortcut operate on files behind it.
        Some(
            if !command_modifiers
                && matches!(
                    key,
                    gdk::Key::Tab
                        | gdk::Key::ISO_Left_Tab
                        | gdk::Key::Home
                        | gdk::Key::End
                        | gdk::Key::Return
                        | gdk::Key::KP_Enter
                        | gdk::Key::space
                )
            {
                glib::Propagation::Proceed
            } else {
                glib::Propagation::Stop
            },
        )
    }

    fn scroll_reference(&self, key: gdk::Key) -> bool {
        let adjustment = self.scroll.vadjustment();
        let page = adjustment.page_size().max(1.0);
        let step = if adjustment.step_increment() >= 1.0 {
            adjustment.step_increment()
        } else {
            page / 10.0
        };
        let page_step = if adjustment.page_increment() >= 1.0 {
            adjustment.page_increment()
        } else {
            page
        };
        let delta = match key {
            gdk::Key::Up | gdk::Key::KP_Up | gdk::Key::Left | gdk::Key::KP_Left => -step,
            gdk::Key::Down | gdk::Key::KP_Down | gdk::Key::Right | gdk::Key::KP_Right => step,
            gdk::Key::Page_Up | gdk::Key::KP_Page_Up => -page_step,
            gdk::Key::Page_Down | gdk::Key::KP_Page_Down => page_step,
            _ => return false,
        };
        let limit = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
        adjustment.set_value((adjustment.value() + delta).clamp(adjustment.lower(), limit));
        true
    }

    fn tilde_toggles(
        &self,
        key: gdk::Key,
        modifiers: gdk::ModifierType,
        reference_open: bool,
    ) -> bool {
        if !super::preferences::PreferenceManager::shared().tenxer_mode() {
            return false;
        }
        if modifiers.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        ) {
            return false;
        }
        let tilde = key == gdk::Key::asciitilde
            || (key == gdk::Key::grave && modifiers.contains(gdk::ModifierType::SHIFT_MASK));
        tilde && (reference_open || !self.prompt_has_focus())
    }
}

fn rebuild_reference(reference: &gtk::Box, mode: BrowserMode) {
    while let Some(child) = reference.first_child() {
        reference.remove(&child);
    }
    for section in super::shortcut_reference::reference_sections(mode) {
        append_section(reference, section.title, &section.rows);
    }
}

/// The footer shows only the pill; the experimental note lives in its tooltip,
/// accessible description, and the shortcut reference.
fn apply_experimental_label(tag: &gtk::Label, reference_note: &gtk::Label, enabled: bool) {
    tag.set_text(crate::ui::tenxer_mode::TAG_TEXT);
    tag.set_visible(enabled);
    let phrase = super::shortcut_reference::EXPERIMENTAL_LABEL;
    reference_note.set_text(if enabled { phrase } else { "" });
    reference_note.set_visible(enabled);
    let announced = if enabled {
        format!("{} {phrase}", crate::ui::tenxer_mode::TAG_NAME)
    } else {
        crate::ui::tenxer_mode::TAG_NAME.to_owned()
    };
    tag.set_tooltip_text(Some(&announced));
    tag.update_property(&[
        gtk::accessible::Property::Label(&announced),
        gtk::accessible::Property::Description(if enabled { phrase } else { "" }),
    ]);
}

const FEEDBACK_FLASH: Duration = Duration::from_millis(2_000);

fn update_visual_mode(label: &gtk::Label, visual: Option<crate::app::VisualKind>) {
    let (text, name) = match visual {
        Some(crate::app::VisualKind::Select) => ("VISUAL", "Visual select"),
        Some(crate::app::VisualKind::Unset) => ("UNSET", "Visual unset"),
        None => ("", ""),
    };
    if label.text() != text {
        label.set_text(text);
        super::accessibility::set_label(label, name);
    }
    label.set_visible(!text.is_empty());
}

#[cfg(test)]
impl ShortcutFooter {
    pub(in crate::ui) fn visual_text(&self) -> Option<String> {
        self.visual
            .is_visible()
            .then(|| self.visual.text().to_string())
    }
}

fn update_item_count(
    label: &gtk::Label,
    browser: &Rc<crate::app::Browser>,
    filter: Option<&FilterStatus>,
) {
    if let Some(filter) = filter {
        let noun = if filter.total() == 1 { "item" } else { "items" };
        label.set_label(&format!("{} {noun}", filter.total()));
        label.set_tooltip_text(Some(&file_folder_breakdown(filter.files, filter.folders)));
        label.set_visible(true);
        return;
    }
    let Some(depth) = browser.active_depth() else {
        label.set_visible(false);
        return;
    };
    let counts = browser.column_entry_counts(depth).unwrap_or_default();
    let selected = browser.selected_entries();
    for position in browser.selected_positions(depth) {
        if let Some(entry) = browser.entry_at(depth, position)
            && !entry.is_directory()
            && entry.size == crate::model::MetadataValue::Unknown
        {
            browser.request_metadata_fill(depth, position, entry.location, false);
        }
    }
    let noun = if counts.total == 1 { "item" } else { "items" };
    if !selected.is_empty() {
        label.set_label(&selection_details(&selected));
        label.set_tooltip_text(Some(&format!(
            "{} of {} {noun} selected. Size includes selected files only; folder contents are not counted.",
            selected.len(), counts.total
        )));
    } else {
        label.set_label(&format!("{} {noun}", counts.total));
        label.set_tooltip_text(Some(&file_folder_breakdown(counts.files, counts.folders)));
    }
    label.set_visible(true);
}

fn file_folder_breakdown(files: usize, folders: usize) -> String {
    let file_noun = if files == 1 { "file" } else { "files" };
    let folder_noun = if folders == 1 { "folder" } else { "folders" };
    format!("{files} {file_noun}, {folders} {folder_noun}")
}

fn selection_details(entries: &[crate::model::FileEntry]) -> String {
    let folders = entries.iter().filter(|entry| entry.is_directory()).count();
    let files = entries.len() - folders;
    let mut parts = Vec::new();
    if folders > 0 {
        let noun = if folders == 1 { "folder" } else { "folders" };
        parts.push(format!("{folders} {noun}"));
    }
    if files > 0 {
        let noun = if files == 1 { "file" } else { "files" };
        parts.push(format!("{files} {noun}"));
    }
    let mut text = format!("{} selected", parts.join(", "));
    if files > 0 {
        let mut bytes = 0u64;
        let mut known = 0;
        for entry in entries.iter().filter(|entry| !entry.is_directory()) {
            if let crate::model::MetadataValue::Known(size) = entry.size {
                bytes = bytes.saturating_add(size);
                known += 1;
            }
        }
        let size = super::browser::format_file_size(bytes);
        if known == files {
            text.push_str(&format!(" ({size})"));
        } else if known > 0 {
            text.push_str(&format!(" ({size} known; size incomplete)"));
        } else {
            text.push_str(" (size unavailable)");
        }
    }
    text
}

fn watch_status_widget(
    widget: &gtk::Widget,
    status_widgets: &Rc<RefCell<Vec<gtk::Widget>>>,
    root: &gtk::Stack,
) {
    let root = root.downgrade();
    let statuses = status_widgets.clone();
    widget.connect_visible_notify(move |_| {
        // Ignore ancestor visibility so a hidden footer can reveal itself.
        if let Some(root) = root.upgrade() {
            root.set_visible(
                statuses
                    .borrow()
                    .iter()
                    .any(gtk::prelude::WidgetExt::get_visible),
            );
        }
    });
}

fn refresh_paste_availability(
    clipboard: &gdk::Clipboard,
    label: &glib::WeakRef<gtk::Label>,
    generation: &Rc<Cell<u64>>,
) {
    let revision = generation.get().wrapping_add(1);
    generation.set(revision);
    let Some(paste) = label.upgrade() else {
        return;
    };
    paste.set_visible(false);
    let formats = clipboard.formats();
    if !formats.contains_type(gdk::FileList::static_type())
        && !formats.contain_mime_type("text/uri-list")
    {
        return;
    }
    let clipboard = clipboard.clone();
    let label = label.clone();
    let generation = generation.clone();
    glib::MainContext::default().spawn_local(async move {
        let available = clipboard
            .read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT)
            .await
            .ok()
            .and_then(|value| value.get::<gdk::FileList>().ok())
            .is_some_and(|files| !files.files().is_empty());
        if revision == generation.get()
            && let Some(label) = label.upgrade()
        {
            label.set_visible(available);
        }
    });
}

fn append_section(parent: &gtk::Box, title: &str, shortcuts: &[Shortcut]) {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 7);
    let heading = gtk::Label::builder().label(title).xalign(0.0).build();
    heading.add_css_class("shortcut-reference-heading");
    section.append(&heading);
    for (key, action) in shortcuts {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        let key = gtk::Label::builder()
            .label(*key)
            .xalign(0.0)
            .width_chars(17)
            .build();
        key.add_css_class("shortcut-reference-key");
        let action = gtk::Label::builder()
            .label(*action)
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .build();
        action.add_css_class("shortcut-reference-description");
        row.append(&key);
        row.append(&action);
        section.append(&row);
    }
    parent.append(&section);
}

#[cfg(test)]
mod tests;
