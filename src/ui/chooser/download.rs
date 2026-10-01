// SPDX-License-Identifier: MIT

use std::{cell::Cell, rc::Rc, time::Duration};

use gtk::{glib, prelude::*};

use crate::ui::browser::format_file_size;

pub(super) struct DownloadProgress {
    pub(super) root: gtk::Box,
    name: gtk::Label,
    status: gtk::Label,
    progress: gtk::ProgressBar,
    indeterminate: Rc<Cell<bool>>,
    pulse_source: Option<glib::SourceId>,
}

impl DownloadProgress {
    pub(super) fn new(url: &str, on_cancel: Rc<dyn Fn()>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("chooser-download");
        root.append(&crate::assets::primary_icon(
            crate::assets::icons::DOWNLOADS,
            16,
        ));
        let name = gtk::Label::new(Some(
            &crate::services::remote_file_name(url).unwrap_or_else(|| "Download".to_owned()),
        ));
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        name.set_max_width_chars(28);
        root.append(&name);
        let progress = gtk::ProgressBar::new();
        progress.add_css_class("job-progress");
        progress.set_size_request(120, -1);
        progress.set_valign(gtk::Align::Center);
        root.append(&progress);
        let status = gtk::Label::new(Some("Connecting…"));
        status.add_css_class("chooser-download-status");
        root.append(&status);
        let cancel = gtk::Button::new();
        cancel.add_css_class("job-action");
        cancel.set_tooltip_text(Some("Cancel download"));
        crate::ui::accessibility::set_label(&cancel, "Cancel download");
        cancel.set_child(Some(&crate::assets::primary_icon(
            crate::assets::icons::X,
            14,
        )));
        cancel.connect_clicked(move |_| on_cancel());
        root.append(&cancel);
        let indeterminate = Rc::new(Cell::new(true));
        let pulsing = indeterminate.clone();
        let weak_progress = progress.downgrade();
        let pulse_source = glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(progress) = weak_progress.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if pulsing.get() {
                progress.pulse();
            }
            glib::ControlFlow::Continue
        });
        Self {
            root,
            name,
            status,
            progress,
            indeterminate,
            pulse_source: Some(pulse_source),
        }
    }

    pub(super) fn set_name(&self, name: &str) {
        self.name.set_text(name);
    }

    pub(super) fn update(&self, downloaded: u64, total: Option<u64>) {
        match total.filter(|total| *total > 0) {
            Some(total) => {
                let fraction = (downloaded as f64 / total as f64).clamp(0.0, 1.0);
                self.status.set_text(&format!(
                    "{}% of {}",
                    (fraction * 100.0) as usize,
                    format_file_size(total)
                ));
                self.indeterminate.set(false);
                self.progress.set_fraction(fraction);
            }
            None => {
                self.status
                    .set_text(&format!("{} downloaded", format_file_size(downloaded)));
                self.indeterminate.set(true);
            }
        }
    }
}

impl Drop for DownloadProgress {
    fn drop(&mut self) {
        if let Some(source) = self.pulse_source.take() {
            source.remove();
        }
    }
}
