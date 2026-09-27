// SPDX-License-Identifier: MIT

//! Keyboard ownership for 10xer mode: the listing hands keys to the drawer and
//! each preview surface decides what those keys do. The content box becomes
//! focusable only while it owns keys, so default Tab order is unchanged.

use super::*;
use crate::ui::browser::{BrowserView, WeakBrowserView};

const OWNER_CLASS: &str = "preview-keyboard-owner";
const MAX_CLAIM_FRAMES: u32 = 30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum PreviewSurface {
    Document,
    Archive,
    Media,
    Text,
    Control,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum DocumentScroll {
    Line(i32),
    HalfPage(i32),
    Page(i32),
    Start,
    End,
}

impl PreviewDrawer {
    /// The browser whose Miller columns yield their keyboard-destination bar
    /// while the drawer owns the keys.
    pub(in crate::ui) fn bind_keyboard_view(&self, view: &BrowserView) {
        self.state.keyboard_view.replace(Some(view.downgrade()));
    }

    /// Whether keyboard focus is anywhere inside the drawer.
    pub(in crate::ui) fn owns_focus(&self, focused: Option<&gtk::Widget>) -> bool {
        let pane = self.state.pane.upcast_ref::<gtk::Widget>();
        focused.is_some_and(|focused| focused == pane || focused.is_ancestor(pane))
    }

    pub(in crate::ui) fn surface(&self, focused: &gtk::Widget) -> PreviewSurface {
        self.state.surface(focused)
    }

    /// Moves keys into the open drawer, waiting for a newly revealed pane to map.
    pub(in crate::ui) fn take_keyboard(&self) -> bool {
        if !self.is_enabled() || self.state.sizing.is_suspended() {
            return false;
        }
        if self.state.claim_keyboard() {
            return true;
        }
        let weak = Rc::downgrade(&self.state);
        let frames = Cell::new(0);
        self.state.pane.add_tick_callback(move |_, _| {
            let Some(state) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            frames.set(frames.get() + 1);
            if !state.is_enabled() || state.claim_keyboard() || frames.get() >= MAX_CLAIM_FRAMES {
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
        true
    }

    pub(in crate::ui) fn scroll_document(&self, motion: DocumentScroll) -> bool {
        self.state.scroll_document(motion)
    }

    /// Plain media keys for a keyboard-owned media preview.
    pub(in crate::ui) fn media_key(&self, key: gtk::gdk::Key) -> bool {
        self.has_video() && self.state.media_command(key)
    }

    pub(in crate::ui) fn archive_at_root(&self) -> bool {
        self.state
            .archive_browser
            .borrow()
            .as_ref()
            .is_none_or(archive::ArchiveBrowser::at_root)
    }

    pub(in crate::ui) fn archive_edge(&self, last: bool) -> bool {
        self.state
            .archive_browser
            .borrow()
            .as_ref()
            .is_some_and(|browser| {
                browser.move_cursor(if last {
                    isize::MAX / 2
                } else {
                    -isize::MAX / 2
                })
            })
    }

    #[cfg(test)]
    pub(in crate::ui) fn document_scroll_value(&self) -> Option<f64> {
        self.state
            .primary_scroll()
            .map(|scroll| scroll.vadjustment().value())
    }
}

impl PreviewState {
    pub(super) fn install_keyboard_ownership(self: &Rc<Self>) {
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(state) = weak.upgrade() {
                state.content.set_focusable(false);
                state.set_keyboard_owner(false);
            }
        });
        self.pane.add_controller(focus);
    }

    /// The owner bar replaces the Miller column's destination bar while the
    /// drawer holds the keys.
    fn set_keyboard_owner(&self, owned: bool) {
        if owned {
            self.pane.add_css_class(OWNER_CLASS);
        } else {
            self.pane.remove_css_class(OWNER_CLASS);
        }
        if let Some(view) = self
            .keyboard_view
            .borrow()
            .as_ref()
            .and_then(WeakBrowserView::upgrade)
        {
            view.set_preview_owns_keys(owned);
        }
    }

    fn claim_keyboard(&self) -> bool {
        if !self.pane.is_mapped() {
            return false;
        }
        let target: gtk::Widget = if let Some(browser) = self.archive_browser.borrow().as_ref() {
            browser.list().clone().upcast()
        } else if let Some(entry) = self.password_entry.borrow().as_ref() {
            entry.clone().upcast()
        } else {
            self.content.set_focusable(true);
            self.content.clone().upcast()
        };
        if !target.grab_focus() {
            return false;
        }
        self.set_keyboard_owner(true);
        true
    }

    /// Replacing content destroys its focused child; keep the keys in the drawer
    /// instead of letting them fall to an unrelated widget.
    pub(super) fn content_owns_keys(&self) -> bool {
        self.pane.has_css_class(OWNER_CLASS)
            && self
                .content
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|focused| {
                    focused == *self.content.upcast_ref::<gtk::Widget>()
                        || focused.is_ancestor(&self.content)
                })
    }

    pub(super) fn keep_keys_in_content(&self, owned: bool) {
        if !owned || !self.is_enabled() {
            return;
        }
        let focus_lost = self
            .content
            .root()
            .and_then(|root| root.focus())
            .is_none_or(|focused| {
                focused != *self.content.upcast_ref::<gtk::Widget>()
                    && !focused.is_ancestor(&self.content)
            });
        if focus_lost {
            self.content.set_focusable(true);
            self.content.grab_focus();
            self.set_keyboard_owner(true);
        }
    }

    /// A newly rendered interactive surface inherits keys owned by the content box.
    pub(super) fn hand_keys_to(&self, widget: &impl IsA<gtk::Widget>) {
        if self.content.has_focus() {
            widget.grab_focus();
        }
    }

    fn surface(&self, focused: &gtk::Widget) -> PreviewSurface {
        if self.archive_list_has_focus(Some(focused)) {
            return PreviewSurface::Archive;
        }
        if crate::ui::focus_navigation::editable(focused) {
            return PreviewSurface::Text;
        }
        let media_view = self.media.borrow().is_some()
            && (focused == self.content.upcast_ref::<gtk::Widget>()
                || focused.is::<gtk::Overlay>());
        if media_view {
            return PreviewSurface::Media;
        }
        let control = focused.is::<gtk::Button>()
            || focused.is::<gtk::Range>()
            || focused.is::<gtk::Switch>()
            || focused.is::<gtk::DropDown>()
            || focused.ancestor(gtk::Button::static_type()).is_some()
            || focused.ancestor(gtk::Range::static_type()).is_some();
        if control {
            PreviewSurface::Control
        } else {
            PreviewSurface::Document
        }
    }

    /// The tallest scrollable view in the content, which is the document body
    /// rather than a breadcrumb strip or metadata scroller.
    fn primary_scroll(&self) -> Option<gtk::ScrolledWindow> {
        fn visit(widget: &gtk::Widget, best: &mut Option<gtk::ScrolledWindow>) {
            if !widget.is_visible() {
                return;
            }
            if let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>()
                && scroll.vscrollbar_policy() != gtk::PolicyType::Never
                && best
                    .as_ref()
                    .is_none_or(|best| scroll.height() > best.height())
            {
                best.replace(scroll.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                visit(&widget, best);
                child = widget.next_sibling();
            }
        }
        let mut best = None;
        visit(self.content.upcast_ref(), &mut best);
        best
    }

    fn scroll_document(&self, motion: DocumentScroll) -> bool {
        let Some(scroll) = self.primary_scroll() else {
            return false;
        };
        let adjustment = scroll.vadjustment();
        let page = adjustment.page_size().max(1.0);
        let lower = adjustment.lower();
        let limit = (adjustment.upper() - adjustment.page_size()).max(lower);
        let target = match motion {
            DocumentScroll::Line(direction) => {
                let step = if adjustment.step_increment() >= 1.0 {
                    adjustment.step_increment()
                } else {
                    page / 10.0
                };
                adjustment.value() + f64::from(direction) * step
            }
            DocumentScroll::HalfPage(direction) => {
                adjustment.value() + f64::from(direction) * page / 2.0
            }
            DocumentScroll::Page(direction) => adjustment.value() + f64::from(direction) * page,
            DocumentScroll::Start => lower,
            DocumentScroll::End => limit,
        };
        adjustment.set_value(target.clamp(lower, limit));
        true
    }
}
