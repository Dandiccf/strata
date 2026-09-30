// SPDX-License-Identifier: MIT

use std::time::Instant;

use super::*;

fn pump(duration: Duration) {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn tooltip(window: &gtk::Window) -> Option<popup::Tooltip> {
    let mut child = window.first_child();
    while let Some(widget) = child {
        if let Ok(tooltip) = widget.clone().downcast::<popup::Tooltip>() {
            return Some(tooltip);
        }
        child = widget.next_sibling();
    }
    None
}

fn move_over(window: &gtk::Window, widget: &impl IsA<gtk::Widget>, hover: &Rc<RefCell<Hover>>) {
    let bounds = widget.compute_bounds(window).expect("pointer target");
    moved(
        window,
        hover,
        f64::from(bounds.x() + bounds.width() / 2.0),
        f64::from(bounds.y() + bounds.height() / 2.0),
        None,
    );
}

#[test]
fn global_tooltips_appear_after_one_delay_in_existing_new_and_rebuilt_views() {
    crate::test_support::gtk_test(
        "ui::tooltips::tests::global_tooltips_appear_after_one_delay_in_existing_new_and_rebuilt_views",
        || {
            let existing = gtk::Window::new();
            let hover = Rc::new(RefCell::new(Hover::default()));
            native::observe(&hover);
            let dialog = gtk::Window::new();
            for window in [&existing, &dialog] {
                window.set_default_size(400, 200);
                for (name, sensitive) in [
                    ("Initial disabled control", false),
                    ("Rebuilt control", true),
                ] {
                    let button = gtk::Button::with_label(name);
                    button.set_tooltip_text(Some(name));
                    button.set_sensitive(sensitive);
                    window.set_child(Some(&button));
                    window.present();
                    pump(Duration::from_millis(100));
                    let focus = gtk::prelude::RootExt::focus(window);
                    let started = Instant::now();
                    move_over(window, &button, &hover);
                    pump(Duration::from_millis(200));
                    assert!(
                        tooltip(window).is_none(),
                        "no early tooltip, including insensitive controls"
                    );
                    let deadline = started + Duration::from_millis(900);
                    while tooltip(window).is_none() && Instant::now() < deadline {
                        pump(Duration::from_millis(5));
                    }
                    let popup = tooltip(window).expect("one 500 ms delay, not a second GTK delay");
                    assert!(started.elapsed() >= HOVER_DELAY);
                    assert!(popup.is_mapped());
                    let label = popup
                        .child()
                        .expect("tooltip content")
                        .downcast::<gtk::Label>()
                        .expect("tooltip label");
                    assert_eq!(label.text(), name);
                    assert_eq!(
                        gtk::prelude::RootExt::focus(window),
                        focus,
                        "tooltips must not take keyboard focus"
                    );
                    let native_tooltip: gtk::Tooltip = glib::Object::new();
                    assert!(
                        !button.emit_by_name::<bool>(
                            "query-tooltip",
                            &[&0_i32, &0_i32, &false, &native_tooltip]
                        ),
                        "GTK must not show a duplicate pointer tooltip"
                    );
                    assert!(
                        button.emit_by_name::<bool>(
                            "query-tooltip",
                            &[&0_i32, &0_i32, &true, &native_tooltip]
                        ),
                        "keyboard tooltips retain the native handler"
                    );
                    window.set_visible(false);
                    assert!(
                        !popup.is_mapped(),
                        "unmapping dismisses an already visible tooltip"
                    );
                    assert!(popup.parent().is_none());
                }
                window.close();
            }
        },
    );
}

#[test]
fn moving_or_leaving_cancels_pending_tooltips_and_markup_is_preserved() {
    crate::test_support::gtk_test(
        "ui::tooltips::tests::moving_or_leaving_cancels_pending_tooltips_and_markup_is_preserved",
        || {
            let window = gtk::Window::new();
            let label = gtk::Label::new(Some("Filename"));
            label.set_tooltip_markup(Some("<b>Folder &amp; files</b>"));
            let unrelated = gtk::Label::new(Some("Other status"));
            unrelated.set_tooltip_text(Some("Other status"));
            let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
            content.append(&label);
            content.append(&unrelated);
            window.set_child(Some(&content));
            window.set_default_size(400, 200);
            let hover = Rc::new(RefCell::new(Hover::default()));
            window.present();
            pump(Duration::from_millis(100));
            move_over(&window, &unrelated, &hover);
            move_over(&window, &label, &hover);
            pump(Duration::from_millis(300));
            move_over(&window, &label, &hover);
            pump(Duration::from_millis(300));
            assert!(tooltip(&window).is_none(), "movement restarts the delay");
            unrelated.set_visible(false);
            pump(Duration::from_millis(300));
            let popup =
                tooltip(&window).expect("an unrelated unmap must not cancel this stationary hover");
            let content = popup
                .child()
                .expect("tooltip content")
                .downcast::<gtk::Label>()
                .expect("tooltip label");
            assert_eq!(content.text(), "Folder & files");
            label.set_tooltip_text(Some("Updated status"));
            assert_eq!(
                content.text(),
                "Updated status",
                "visible tooltips follow live content changes"
            );
            move_over(&window, &label, &hover);
            assert!(!popup.is_mapped(), "moving dismisses the previous tooltip");
            pump(HOVER_DELAY + Duration::from_millis(100));
            let popup = tooltip(&window).expect("tooltip after hovering again");
            label.set_has_tooltip(false);
            assert!(
                !popup.is_mapped(),
                "disabling a tooltip dismisses it immediately"
            );
            label.set_has_tooltip(true);
            move_over(&window, &label, &hover);
            cancel_for(window.upcast_ref(), &hover);
            pump(HOVER_DELAY + Duration::from_millis(100));
            assert!(
                tooltip(&window).is_none(),
                "leaving cancels a pending tooltip"
            );
            move_over(&window, &label, &hover);
            window.set_visible(false);
            pump(HOVER_DELAY + Duration::from_millis(100));
            assert!(
                tooltip(&window).is_none(),
                "a pending tooltip does not reopen a hidden window"
            );
            window.close();
        },
    );
}
