//! The capture bar: one line above the game while
//! Caskade saves it, in GTK, outside the page.

use gtk::prelude::*;

/// What the bar's buttons ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Done while playing; Close once the check passed.
    Primary,
    /// Play a bit more, after a check found missing parts.
    PlayMore,
    /// Save anyway, after a check found missing parts.
    SaveAnyway,
}

/// The bar's height, margins included, in logical pixels.
pub const HEIGHT: i32 = 46;

#[derive(Clone)]
pub struct Bar {
    text: gtk::Label,
    stats: gtk::Label,
    primary: gtk::Button,
    more: gtk::Button,
    save: gtk::Button,
}

impl Bar {
    /// Adds the bar at the top of `container`, before the page is added.
    pub fn new(container: &gtk::Box, send: impl Fn(Action) + Clone + 'static) -> Self {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.set_margin_start(12);
        row.set_margin_end(12);
        row.set_margin_top(6);
        row.set_margin_bottom(6);
        // As tall with its buttons hidden, so the page doesn't move.
        row.set_size_request(-1, HEIGHT - 12);
        let text = gtk::Label::new(None);
        let stats = gtk::Label::new(None);
        stats.style_context().add_class("dim-label");
        let primary = gtk::Button::with_label("Done");
        primary.style_context().add_class("suggested-action");
        let more = gtk::Button::with_label("Play a bit more");
        let save = gtk::Button::with_label("Save anyway");
        for (button, action) in [
            (&primary, Action::Primary),
            (&more, Action::PlayMore),
            (&save, Action::SaveAnyway),
        ] {
            let send = send.clone();
            button.connect_clicked(move |_| send(action));
        }
        row.pack_start(&text, false, false, 0);
        row.pack_start(&stats, false, false, 0);
        row.pack_end(&primary, false, false, 0);
        row.pack_end(&save, false, false, 0);
        row.pack_end(&more, false, false, 0);
        container.pack_start(&row, false, false, 0);
        row.show_all();
        let bar = Self {
            text,
            stats,
            primary,
            more,
            save,
        };
        bar.capturing();
        bar
    }

    /// While the player plays and Caskade keeps what the game loads.
    pub fn capturing(&self) {
        self.show(
            "Play for a minute so Caskade can save the game.",
            Some("Done"),
            false,
        );
        self.stats.show();
    }

    /// Still capturing, once the game has been found in its page.
    pub fn found(&self) {
        self.text.set_text(
            "Found the game: only it will be kept. Play for a minute so Caskade can save it.",
        );
    }

    /// While the bundle plays without the network.
    pub fn checking(&self) {
        self.show("Checking it works offline…", None, false);
        self.stats.hide();
    }

    pub fn ready(&self) {
        self.show("Ready to play offline.", Some("Close"), false);
    }

    pub fn missing(&self) {
        self.show("A few parts are still missing.", None, true);
    }

    /// How much has been kept: `142 files · 38 MB`.
    pub fn stats(&self, files: usize, bytes: u64) {
        let plural = if files == 1 { "file" } else { "files" };
        self.stats
            .set_text(&format!("{files} {plural} · {}", size(bytes)));
    }

    fn show(&self, text: &str, primary: Option<&str>, choices: bool) {
        self.text.set_text(text);
        match primary {
            Some(label) => {
                self.primary.set_label(label);
                self.primary.show();
            }
            None => self.primary.hide(),
        }
        self.more.set_visible(choices);
        self.save.set_visible(choices);
    }
}

/// Bytes for people: `840 KB`, `38 MB`, `1.2 GB`.
fn size(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    let bytes = bytes as f64;
    if bytes < KB * KB {
        format!("{:.0} KB", (bytes / KB).max(1.0))
    } else if bytes < KB * KB * KB {
        format!("{:.0} MB", bytes / (KB * KB))
    } else {
        format!("{:.1} GB", bytes / (KB * KB * KB))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_read_as_kilobytes_megabytes_and_gigabytes() {
        assert_eq!(super::size(12), "1 KB");
        assert_eq!(super::size(840_000), "840 KB");
        assert_eq!(super::size(38_200_000), "38 MB");
        assert_eq!(super::size(1_240_000_000), "1.2 GB");
    }
}
