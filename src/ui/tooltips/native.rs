// SPDX-License-Identifier: MIT

use std::ffi::c_void;

use glib::{subclass::SignalId, translate::*};

use super::*;

#[expect(
    unsafe_code,
    reason = "GLib emission hooks have no safe gtk-rs wrapper"
)]
pub(super) fn observe(hover: &Rc<RefCell<Hover>>) {
    let _class = glib::Class::<gtk::gdk::Surface>::from_type(gtk::gdk::Surface::static_type())
        .expect("Surface class");
    let signal =
        SignalId::lookup("event", gtk::gdk::Surface::static_type()).expect("Surface event signal");
    let state = Box::into_raw(Box::new(hover.clone()));
    // SAFETY: GDK dispatches input on GTK's main thread. GLib owns the boxed Rc for
    // this process-wide hook's lifetime and supplies borrowed signal values.
    let id = unsafe {
        glib::gobject_ffi::g_signal_add_emission_hook(
            signal.into_glib(),
            0,
            Some(input),
            state.cast(),
            Some(destroy),
        )
    };
    assert_ne!(id, 0, "GTK tooltip observation hook unavailable");
}

#[expect(
    unsafe_code,
    reason = "GdkSurface::event supplies a borrowed GdkEvent pointer, not a boxed value"
)]
unsafe extern "C" fn input(
    _: *mut glib::gobject_ffi::GSignalInvocationHint,
    count: u32,
    values: *const glib::gobject_ffi::GValue,
    data: *mut c_void,
) -> glib::ffi::gboolean {
    if count > 1 {
        // SAFETY: count includes the emitting surface and its event parameter;
        // GLib keeps both values alive throughout this hook invocation.
        let value = unsafe { glib::Value::from_glib_borrow(values.wrapping_add(1)) };
        if let Ok(pointer) = value.get::<glib::Pointer>()
            && !pointer.is_null()
        {
            // SAFETY: GdkSurface::event defines this G_TYPE_POINTER parameter as
            // a borrowed GdkEvent. It remains alive throughout signal emission.
            let event = unsafe { gtk::gdk::Event::from_glib_borrow(pointer.cast()) };
            // SAFETY: observe transfers this boxed Rc to GLib, which retains it
            // until destroy. This callback executes on GTK's main thread.
            let hover = unsafe { &*data.cast::<Rc<RefCell<Hover>>>() };
            observed(&event, hover);
        }
    }
    // Keep observing; never consume, mutate, or replay an input event.
    1
}

fn observed(event: &gtk::gdk::Event, hover: &Rc<RefCell<Hover>>) {
    match event.event_type() {
        gtk::gdk::EventType::ButtonPress
        | gtk::gdk::EventType::ButtonRelease
        | gtk::gdk::EventType::KeyPress
        | gtk::gdk::EventType::Scroll
        | gtk::gdk::EventType::DragEnter
        | gtk::gdk::EventType::GrabBroken => {
            cancel(hover);
        }
        gtk::gdk::EventType::MotionNotify | gtk::gdk::EventType::EnterNotify => {
            if event.event_type() == gtk::gdk::EventType::MotionNotify
                && event.time() == gtk::gdk::CURRENT_TIME
            {
                return;
            }
            let Some((widget, window)) = context(event) else {
                return;
            };
            let Some((x, y)) = event.position() else {
                return;
            };
            let Some(native) = widget.native() else {
                return;
            };
            let (offset_x, offset_y) = native.surface_transform();
            if let Some(point) = widget.compute_point(
                &window,
                &gtk::graphene::Point::new((x - offset_x) as f32, (y - offset_y) as f32),
            ) {
                moved(
                    &window,
                    hover,
                    f64::from(point.x()),
                    f64::from(point.y()),
                    Some(event),
                );
            }
        }
        gtk::gdk::EventType::LeaveNotify => {
            if let Some((widget, _)) = context(event) {
                cancel_for(&widget, hover);
            }
        }
        _ => {}
    }
}

fn context(event: &gtk::gdk::Event) -> Option<(gtk::Widget, gtk::Window)> {
    let widget = gtk::Native::for_surface(&event.surface()?)?
        .dynamic_cast::<gtk::Widget>()
        .ok()?;
    if widget.is::<popup::Tooltip>() {
        return None;
    }
    let window = widget.root().and_downcast::<gtk::Window>()?;
    Some((widget, window))
}

#[expect(
    unsafe_code,
    reason = "Release the boxed Rc transferred to GLib at registration"
)]
unsafe extern "C" fn destroy(data: *mut c_void) {
    // SAFETY: GLib invokes this destructor once for the allocation in observe.
    drop(unsafe { Box::from_raw(data.cast::<Rc<RefCell<Hover>>>()) });
}
