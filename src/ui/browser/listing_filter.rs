// SPDX-License-Identifier: MIT

//! 10xer footer filter. It drives the focused pane's own filter field without
//! revealing the funnel, so that field's name rules, **Include subfolders**
//! scope, and stale-work cancellation stay the filter's rules.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gtk::{glib, prelude::*};

use super::{BrowserView, ViewState, columns::ColumnView};
use crate::{
    services::SearchItem,
    ui::{browser_modes::BrowserMode, inline_search::InlineSearch},
};

#[derive(Default)]
pub(super) struct FilterState {
    owner: RefCell<Weak<ViewState>>,
    handlers: RefCell<Vec<Rc<dyn Fn()>>>,
    /// A commit whose results were still loading; their arrival focuses the
    /// first result.
    focus_on_arrival: Cell<bool>,
    notify_scheduled: Cell<bool>,
}

impl FilterState {
    pub(super) fn set_owner(&self, state: &Rc<ViewState>) {
        self.owner.replace(Rc::downgrade(state));
    }
}

/// The focused listing's filter as the footer reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct FilterStatus {
    pub query: String,
    pub files: usize,
    pub folders: usize,
}

impl FilterStatus {
    pub(in crate::ui) fn total(&self) -> usize {
        self.files + self.folders
    }
}

/// The result `steps` rows from `current`: zero stays (on the first result
/// without a cursor) and `usize::MAX` jumps to an end.
pub(in crate::ui) fn results_step_target(
    current: Option<u32>,
    count: u32,
    direction: i32,
    steps: usize,
) -> Option<u32> {
    let last = count.checked_sub(1)?;
    if steps == 0 {
        return Some(current.map_or(0, |current| current.min(last)));
    }
    if steps == usize::MAX {
        return Some(if direction < 0 { 0 } else { last });
    }
    let steps = u32::try_from(steps).unwrap_or(u32::MAX);
    Some(match current {
        Some(current) if direction < 0 => current.saturating_sub(steps),
        Some(current) => current.saturating_add(steps).min(last),
        None if direction < 0 => last,
        None => 0,
    })
}

pub(in crate::ui) fn scroll_results_to(
    view: &gtk::Widget,
    position: u32,
    flags: gtk::ListScrollFlags,
) {
    if let Some(list) = view.downcast_ref::<gtk::ListView>() {
        list.scroll_to(position, flags, None);
    } else if let Some(grid) = view.downcast_ref::<gtk::GridView>() {
        grid.scroll_to(position, flags, None);
    }
}

/// Where the focused listing keeps its filter.
enum Target {
    Column(Box<ColumnView>),
    Pane {
        entry: gtk::Entry,
        button: gtk::ToggleButton,
        search: InlineSearch,
    },
}

impl Target {
    fn entry(&self) -> &gtk::Entry {
        match self {
            Self::Column(column) => &column.filter_entry,
            Self::Pane { entry, .. } => entry,
        }
    }

    fn button(&self) -> &gtk::ToggleButton {
        match self {
            Self::Column(column) => &column.filter_button,
            Self::Pane { button, .. } => button,
        }
    }

    fn flush(&self) {
        match self {
            Self::Column(column) => column.flush_filter_query(),
            Self::Pane { search, .. } => search.flush_query(),
        }
    }

    /// The view showing results in place of the directory, if any.
    fn results_view(&self) -> Option<gtk::Widget> {
        match self {
            Self::Column(column) => column
                .recursive_search_active
                .get()
                .then(|| column.list.clone().upcast()),
            Self::Pane { search, .. } => search.results_view(),
        }
    }

    fn results(&self) -> Option<Vec<SearchItem>> {
        match self {
            Self::Column(column) => column
                .recursive_search_active
                .get()
                .then(|| column.search_results.borrow().clone()),
            Self::Pane { search, .. } => search.results(),
        }
    }

    fn step(&self, direction: i32, steps: usize, take_focus: bool) -> bool {
        match self {
            Self::Column(column) => step_column_results(column, direction, steps, take_focus),
            Self::Pane { search, .. } => search.step(direction, steps, take_focus),
        }
    }

    fn invert(&self) -> bool {
        match self {
            Self::Column(column) => {
                if !column.recursive_search_active.get() {
                    return false;
                }
                let count = column.selection.n_items();
                if count == 0 {
                    return false;
                }
                let inverted = gtk::Bitset::new_range(0, count);
                inverted.subtract(&column.selection.selection());
                column
                    .selection
                    .set_selection(&inverted, &gtk::Bitset::new_range(0, count));
                true
            }
            Self::Pane { search, .. } => search.invert_selection(),
        }
    }
}

fn column_cursor(column: &ColumnView) -> Option<u32> {
    let focused = column.list.root().and_then(|root| root.focus());
    focused
        .and_then(|focused| {
            column.bound_rows.borrow().iter().find_map(|bound| {
                let row = bound.row.upgrade()?;
                (focused == *row.upcast_ref::<gtk::Widget>() || focused.is_ancestor(&row))
                    .then(|| bound.item.upgrade().map(|item| item.position()))
                    .flatten()
            })
        })
        .or_else(|| {
            let selected = column.selection.selection();
            (!selected.is_empty()).then(|| selected.maximum())
        })
}

fn step_column_results(
    column: &ColumnView,
    direction: i32,
    steps: usize,
    take_focus: bool,
) -> bool {
    if !column.recursive_search_active.get() {
        return false;
    }
    let Some(target) = results_step_target(
        column_cursor(column),
        column.selection.n_items(),
        direction,
        steps,
    ) else {
        return true;
    };
    column.selection.select_item(target, true);
    let flags = if take_focus {
        column.list.grab_focus();
        gtk::ListScrollFlags::FOCUS
    } else {
        gtk::ListScrollFlags::NONE
    };
    column.list.scroll_to(target, flags, None);
    true
}

fn filter_status(target: &Target) -> Option<FilterStatus> {
    let query = target.entry().text();
    if query.trim().is_empty() {
        return None;
    }
    let results = target.results().unwrap_or_default();
    let folders = results.iter().filter(|item| item.is_directory).count();
    Some(FilterStatus {
        query: query.to_string(),
        files: results.len() - folders,
        folders,
    })
}

fn set_filter_text(target: &Target, query: &str) {
    if query.is_empty() {
        // Closing a funnel that 10xer hides clears nothing; the text is cleared below.
        target.button().set_active(false);
    }
    if target.entry().text() != query {
        target.entry().set_text(query);
    }
}

impl ViewState {
    fn filter_target(&self) -> Option<Target> {
        self.try_filter_target().flatten()
    }

    /// `None` while a rebuild holds the views, which browser events can
    /// interrupt.
    fn try_filter_target(&self) -> Option<Option<Target>> {
        if self.mode.get() == BrowserMode::Columns {
            let columns = self.columns.try_borrow().ok()?;
            let depth = self
                .focused_column_depth()
                .or_else(|| self.browser.active_depth());
            return Some(depth.and_then(|depth| {
                columns
                    .get(depth)
                    .cloned()
                    .map(|column| Target::Column(Box::new(column)))
            }));
        }
        let panes = self.mode_views.try_borrow().ok()?;
        Some(
            panes
                .active_filter()
                .map(|(entry, button, search)| Target::Pane {
                    entry,
                    button,
                    search,
                }),
        )
    }
}

impl BrowserView {
    fn filter_target(&self) -> Option<Target> {
        self.state.filter_target()
    }

    /// The focused listing's filter text, whether or not its funnel is shown.
    pub(in crate::ui) fn listing_filter(&self) -> Option<String> {
        let text = self.filter_target()?.entry().text();
        (!text.trim().is_empty()).then(|| text.to_string())
    }

    /// Filters the focused listing by `query` (an empty query clears it) and
    /// leaves the funnel closed.
    pub(in crate::ui) fn set_listing_filter(&self, query: &str) {
        let Some(target) = self.filter_target() else {
            return;
        };
        if let Target::Column(column) = &target {
            // Columns filter one pane at a time.
            let others: Vec<_> = self
                .state
                .columns
                .borrow()
                .iter()
                .filter(|other| other.filter_entry != column.filter_entry)
                .map(|other| Target::Column(Box::new(other.clone())))
                .collect();
            for other in others
                .iter()
                .filter(|other| !other.entry().text().is_empty())
            {
                set_filter_text(other, "");
            }
        }
        set_filter_text(&target, query);
        self.state.notify_filter_results_changed();
    }

    /// Applies `query` at once and returns keyboard focus to the filtered
    /// listing without opening an item.
    pub(in crate::ui) fn commit_listing_filter(&self, query: &str) {
        self.set_listing_filter(query);
        let Some(target) = self.filter_target() else {
            return;
        };
        target.flush();
        self.keyboard_navigation();
        let Some(view) = target.results_view() else {
            self.state.browser.focus_active();
            return;
        };
        if target.selection_is_empty() {
            self.state.listing_filter.focus_on_arrival.set(true);
            view.grab_focus();
        } else {
            target.step(1, 0, true);
        }
    }

    /// Clears the focused listing's filter and returns focus to its directory.
    pub(in crate::ui) fn clear_listing_filter(&self) -> bool {
        let Some(target) = self.filter_target() else {
            return false;
        };
        if target.entry().text().is_empty() {
            return false;
        }
        self.state.listing_filter.focus_on_arrival.set(false);
        set_filter_text(&target, "");
        target.flush();
        self.state.notify_filter_results_changed();
        self.keyboard_navigation();
        self.state.browser.focus_active();
        true
    }

    /// Clears the filters 10xer left behind a closed funnel in every pane.
    pub(in crate::ui) fn clear_hidden_filters(&self) {
        self.state.listing_filter.focus_on_arrival.set(false);
        let columns: Vec<_> = self
            .state
            .columns
            .borrow()
            .iter()
            .filter(|column| !column.filter_button.is_active())
            .map(|column| column.filter_entry.clone())
            .collect();
        let panes = self.state.mode_views.borrow().hidden_filter_entries();
        for entry in columns.into_iter().chain(panes) {
            entry.set_text("");
        }
    }

    /// The focused listing's filter; `None` while a rebuild holds the views,
    /// so callers retry rather than report a stale listing.
    pub(in crate::ui) fn filter_status(&self) -> Option<Option<FilterStatus>> {
        Some(
            self.state
                .try_filter_target()?
                .and_then(|target| filter_status(&target)),
        )
    }

    /// Moves among filter results instead of the hidden directory cursor.
    /// Returns whether results are showing.
    pub(in crate::ui) fn step_filter_results(
        &self,
        direction: i32,
        steps: usize,
        take_focus: bool,
    ) -> bool {
        self.filter_target()
            .is_some_and(|target| target.step(direction, steps, take_focus))
    }

    /// Inverts the selection among filter results. Returns whether results are
    /// showing.
    pub(in crate::ui) fn invert_filter_results(&self) -> bool {
        self.filter_target().is_some_and(|target| {
            target.results_view().is_some() && {
                target.invert();
                true
            }
        })
    }

    /// Names of the displayed filter results, in display order.
    #[cfg(test)]
    pub(in crate::ui) fn filter_result_names(&self) -> Vec<String> {
        self.filter_target()
            .and_then(|target| target.results())
            .unwrap_or_default()
            .into_iter()
            .map(|item| item.name)
            .collect()
    }

    pub(in crate::ui) fn connect_filter_results_changed(&self, handler: Rc<dyn Fn()>) {
        self.state
            .listing_filter
            .handlers
            .borrow_mut()
            .push(handler);
    }
}

impl Target {
    fn selection_is_empty(&self) -> bool {
        match self {
            Self::Column(column) => column.selection.n_items() == 0,
            Self::Pane { search, .. } => search.results().is_none_or(|results| results.is_empty()),
        }
    }
}

impl ViewState {
    /// Results can change while a pane is being built with its views borrowed,
    /// so observers run once on idle for any burst of changes.
    pub(in crate::ui) fn notify_filter_results_changed(&self) {
        if self.listing_filter.notify_scheduled.replace(true) {
            return;
        }
        let owner = self.listing_filter.owner.borrow().clone();
        glib::idle_add_local_once(move || {
            let Some(state) = owner.upgrade() else {
                return;
            };
            state.listing_filter.notify_scheduled.set(false);
            if state.listing_filter.focus_on_arrival.get() {
                state.focus_arrived_results();
            }
            let handlers = state.listing_filter.handlers.borrow().clone();
            for handler in handlers {
                handler();
            }
        });
    }

    fn focus_arrived_results(&self) {
        let Some(target) = self.filter_target() else {
            return;
        };
        let Some(results) = target.results_view() else {
            self.listing_filter.focus_on_arrival.set(false);
            return;
        };
        let focus = self.overlay.root().and_then(|root| root.focus());
        let still_waiting = focus
            .as_ref()
            .is_none_or(|focus| focus == &results || focus.is_ancestor(&results));
        if !still_waiting {
            self.listing_filter.focus_on_arrival.set(false);
            return;
        }
        if !target.selection_is_empty() {
            self.listing_filter.focus_on_arrival.set(false);
            target.step(1, 0, true);
        }
    }
}
