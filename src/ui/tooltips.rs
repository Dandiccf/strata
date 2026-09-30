// SPDX-License-Identifier: MIT

use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk::{glib, prelude::*};

mod native;
mod popup;

const HOVER_DELAY: Duration = Duration::from_millis(500);

#[derive(Default)]
struct Hover {
    pending: Option<glib::SourceId>,
    visible: Option<popup::Tooltip>,
    owner: glib::WeakRef<gtk::Widget>,
    candidate: glib::WeakRef<gtk::Widget>,
    revision: u64,
}

fn cancel(hover: &Rc<RefCell<Hover>>) {
    let (pending, visible) = {
        let mut state = hover.borrow_mut();
        state.revision = state.revision.wrapping_add(1);
        state.owner.set(None);
        state.candidate.set(None);
        (state.pending.take(), state.visible.take())
    };
    // GTK can emit nested surface events while hiding a popup.
    if let Some(source) = pending {
        source.remove();
    }
    if let Some(popup) = visible {
        popup.set_visible(false);
        popup.unparent();
    }
}

/// GTK 4 hardcodes both hover and browse timers without a GtkSettings override.
/// Present standard text tooltips ourselves so one timer governs every window.
pub(crate) fn install() {
    native::observe(&Rc::new(RefCell::new(Hover::default())));
}

fn moved(
    window: &gtk::Window,
    hover: &Rc<RefCell<Hover>>,
    x: f64,
    y: f64,
    event: Option<&gtk::gdk::Event>,
) {
    cancel(hover);
    if event.is_some_and(|event| {
        let buttons = gtk::gdk::ModifierType::BUTTON1_MASK
            | gtk::gdk::ModifierType::BUTTON2_MASK
            | gtk::gdk::ModifierType::BUTTON3_MASK
            | gtk::gdk::ModifierType::BUTTON4_MASK
            | gtk::gdk::ModifierType::BUTTON5_MASK;
        event.modifier_state().intersects(buttons)
            || event
                .device()
                .is_some_and(|device| device.source() == gtk::gdk::InputSource::Touchscreen)
    }) {
        return;
    }
    let target = event
        .and_then(|event| {
            let native = gtk::Native::for_surface(&event.surface()?)?;
            let (x, y) = event.position()?;
            let (offset_x, offset_y) = native.surface_transform();
            native.dynamic_cast::<gtk::Widget>().ok()?.pick(
                x - offset_x,
                y - offset_y,
                gtk::PickFlags::INSENSITIVE,
            )
        })
        .or_else(|| window.pick(x, y, gtk::PickFlags::INSENSITIVE));
    let Some(target) = target else {
        return;
    };
    let mut ancestor = Some(target.clone());
    while let Some(widget) = ancestor {
        guard(&widget, hover);
        ancestor = widget.parent();
    }
    hover.borrow_mut().candidate.set(Some(&target));
    let target = target.downgrade();
    let window = window.downgrade();
    let delayed = hover.clone();
    let revision = hover.borrow().revision;
    hover.borrow_mut().pending = Some(glib::timeout_add_local_once(HOVER_DELAY, move || {
        {
            let mut state = delayed.borrow_mut();
            if state.revision != revision {
                return;
            }
            state.pending = None;
        }
        let Some(window) = window.upgrade().filter(|window| window.is_mapped()) else {
            return;
        };
        let Some(candidate) = target.upgrade() else {
            return;
        };
        let Some(parent) = candidate
            .native()
            .and_then(|native| native.dynamic_cast::<gtk::Widget>().ok())
        else {
            return;
        };
        let Some(point) =
            window.compute_point(&parent, &gtk::graphene::Point::new(x as f32, y as f32))
        else {
            return;
        };
        if parent
            .pick(
                f64::from(point.x()),
                f64::from(point.y()),
                gtk::PickFlags::INSENSITIVE,
            )
            .as_ref()
            != Some(&candidate)
        {
            return;
        }
        let mut target = Some(candidate);
        while let Some(widget) = target {
            if !widget.is_mapped() {
                return;
            }
            if let Some(markup) = content_markup(&widget) {
                let popup = popup::Tooltip::new(&markup);
                popup.set_parent(&parent);
                popup.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                    point.x() as i32,
                    point.y() as i32,
                    1,
                    1,
                )));
                popup.set_position(gtk::PositionType::Bottom);
                popup.set_offset(0, 8);
                popup.popup();
                let Some(current) = content_markup(&widget).filter(|_| widget.is_mapped()) else {
                    popup.set_visible(false);
                    popup.unparent();
                    return;
                };
                if current != markup {
                    popup.set_markup(&current);
                }
                let mut state = delayed.borrow_mut();
                if state.revision == revision {
                    state.owner.set(Some(&widget));
                    state.visible = Some(popup);
                } else {
                    drop(state);
                    popup.set_visible(false);
                    popup.unparent();
                }
                return;
            }
            target = widget.parent();
        }
    }));
}

fn guard(widget: &gtk::Widget, hover: &Rc<RefCell<Hover>>) {
    let controllers = widget.observe_controllers();
    if (0..controllers.n_items()).any(|index| {
        controllers
            .item(index)
            .and_downcast::<gtk::EventController>()
            .is_some_and(|controller| controller.name().as_deref() == Some("strata-tooltip-delay"))
    }) {
        return;
    }
    let marker = gtk::EventControllerMotion::new();
    marker.set_name(Some("strata-tooltip-delay"));
    marker.set_propagation_phase(gtk::PropagationPhase::None);
    widget.add_controller(marker);
    widget.connect_query_tooltip(move |widget, _, _, keyboard, _| {
        if !keyboard && widget.tooltip_markup().is_some() {
            // Returning false alone would still invoke GTK's default handler.
            widget.stop_signal_emission_by_name("query-tooltip");
        }
        false
    });
    let updates = hover.clone();
    widget.connect_tooltip_markup_notify(move |widget| refresh(widget, &updates));
    let updates = hover.clone();
    widget.connect_has_tooltip_notify(move |widget| refresh(widget, &updates));
    let hover = hover.clone();
    widget.connect_unmap(move |widget| cancel_for(widget, &hover));
}

fn cancel_for(widget: &gtk::Widget, hover: &Rc<RefCell<Hover>>) {
    let affected = {
        let state = hover.borrow();
        [state.owner.upgrade(), state.candidate.upgrade()]
            .into_iter()
            .flatten()
            .any(|target| target == *widget || target.is_ancestor(widget))
    };
    if affected {
        cancel(hover);
    }
}

fn content_markup(widget: &gtk::Widget) -> Option<glib::GString> {
    if widget.has_tooltip() {
        widget.tooltip_markup().filter(|markup| !markup.is_empty())
    } else {
        None
    }
}

fn refresh(widget: &gtk::Widget, hover: &Rc<RefCell<Hover>>) {
    let popup = {
        let state = hover.borrow();
        if state.owner.upgrade().as_ref() != Some(widget) {
            return;
        }
        state.visible.clone()
    };
    if let Some(popup) = popup {
        if let Some(markup) = content_markup(widget) {
            popup.set_markup(&markup);
        } else {
            cancel(hover);
        }
    }
}

#[cfg(test)]
mod tests;
