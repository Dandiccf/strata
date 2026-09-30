// SPDX-License-Identifier: MIT

use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Tooltip;

    #[glib::object_subclass]
    impl ObjectSubclass for Tooltip {
        const NAME: &'static str = "StrataTooltip";
        type Type = super::Tooltip;
        type ParentType = gtk::Popover;

        fn class_init(class: &mut Self::Class) {
            class.set_accessible_role(gtk::AccessibleRole::Tooltip);
        }
    }

    impl ObjectImpl for Tooltip {}
    impl WidgetImpl for Tooltip {
        fn realize(&self) {
            self.parent_realize();
            if let Some(surface) = self.obj().surface() {
                surface.set_input_region(Some(&gtk::cairo::Region::create()));
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            // Like GTK's native tooltip window, this surface must let pointer
            // input pass through, including after GTK updates its popup shape.
            if let Some(surface) = self.obj().surface() {
                surface.set_input_region(Some(&gtk::cairo::Region::create()));
            }
        }
    }
    impl PopoverImpl for Tooltip {}
}

glib::wrapper! {
    pub struct Tooltip(ObjectSubclass<imp::Tooltip>)
        @extends gtk::Popover, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::ShortcutManager;
}

impl Tooltip {
    pub(super) fn new(markup: &str) -> Self {
        let popup: Self = glib::Object::builder()
            .property("autohide", false)
            .property("has-arrow", false)
            .property("can-focus", false)
            .property("can-target", false)
            .build();
        popup.add_css_class("column-popover");
        popup.add_css_class("strata-tooltip");
        let label = gtk::Label::new(None);
        label.set_markup(markup);
        crate::ui::accessibility::set_label(&popup, &label.text());
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_max_width_chars(80);
        popup.set_child(Some(&label));
        popup
    }

    pub(super) fn set_markup(&self, markup: &str) {
        if let Some(label) = self.child().and_downcast::<gtk::Label>() {
            label.set_markup(markup);
            crate::ui::accessibility::set_label(self, &label.text());
        }
    }
}
