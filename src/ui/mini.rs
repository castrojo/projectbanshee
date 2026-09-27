//! Mini Mode: a compact, artwork-lit capsule. The cover fills the background (blurred),
//! transport sits on the right, a hairline seek bar runs along the bottom, and `+`
//! opens a quick-add search under the capsule so the queue can grow without leaving it.

use crate::app::Controller;
use crate::ui::player_widgets::{Progress, TrackInfo, Transport, icon_button};
use crate::ui::search::SearchPage;
use adw::prelude::*;
use std::rc::Rc;

pub const WIDTH: i32 = 520;
pub const COLLAPSED_HEIGHT: i32 = 118;
pub const EXPANDED_HEIGHT: i32 = 470;

pub struct MiniPlayer {
    pub root: gtk::WindowHandle,
    pub quick_add: Rc<SearchPage>,
    revealer: gtk::Revealer,
    add_toggle: gtk::ToggleButton,
}

impl MiniPlayer {
    pub fn new(ctl: &Rc<Controller>) -> Rc<Self> {
        let info = TrackInfo::new(ctl, 72, true, true);
        let transport = Transport::new(ctl, 42);
        let progress = Progress::new(ctl, false);
        progress.root.add_css_class("mini-progress");

        let add_toggle = gtk::ToggleButton::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add to Queue")
            .valign(gtk::Align::Center)
            .build();
        add_toggle.add_css_class("flat");
        add_toggle.add_css_class("circular");
        add_toggle.update_property(&[gtk::accessible::Property::Label("Add to queue")]);
        let leave = icon_button("view-fullscreen-symbolic", "Leave Mini Mode");
        leave.set_action_name(Some("win.mini-mode"));
        let side = gtk::Box::new(gtk::Orientation::Vertical, 2);
        side.set_valign(gtk::Align::Center);
        side.append(&add_toggle);
        side.append(&leave);

        let top = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        top.append(&info.root);
        top.append(&transport.root);
        top.append(&side);
        top.set_margin_top(14);
        top.set_margin_start(14);
        top.set_margin_end(10);
        top.set_margin_bottom(6);

        let quick_add = SearchPage::new(ctl, true);
        quick_add.root.add_css_class("mini-quick-add");
        quick_add.root.set_vexpand(true);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(180)
            .child(&quick_add.root)
            .reveal_child(false)
            .vexpand(true)
            .build();

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("mini-content");
        content.append(&top);
        content.append(&progress.root);
        content.append(&revealer);

        let scrim = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scrim.add_css_class("mini-scrim");
        let overlay = gtk::Overlay::builder()
            .child(&info.backdrop)
            .overflow(gtk::Overflow::Hidden)
            .build();
        overlay.add_overlay(&scrim);
        overlay.add_overlay(&content);
        overlay.set_measure_overlay(&content, true);
        overlay.add_css_class("mini-capsule");
        overlay.add_css_class("banshee-accent");

        let root = gtk::WindowHandle::builder().child(&overlay).build();
        let this = Rc::new(Self {
            root,
            quick_add,
            revealer,
            add_toggle,
        });

        let weak = Rc::downgrade(&this);
        this.add_toggle.connect_toggled(move |t| {
            if let Some(m) = weak.upgrade() {
                m.set_expanded(t.is_active());
            }
        });
        // Escape in the quick-add closes it again.
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&this);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(m) = weak.upgrade() {
                    if m.add_toggle.is_active() {
                        m.add_toggle.set_active(false);
                        return glib::Propagation::Stop;
                    }
                }
            }
            glib::Propagation::Proceed
        });
        this.root.add_controller(keys);
        this
    }

    /// Open the quick-add search (Ctrl+F / Ctrl+L in Mini Mode).
    pub fn open_quick_add(&self) {
        if !self.add_toggle.is_active() {
            self.add_toggle.set_active(true);
        } else {
            self.quick_add.focus();
        }
    }

    pub fn collapse(&self) {
        self.add_toggle.set_active(false);
    }

    fn set_expanded(&self, on: bool) {
        self.revealer.set_reveal_child(on);
        if let Some(w) = self.root.root().and_downcast::<gtk::Window>() {
            w.set_default_size(
                WIDTH,
                if on {
                    EXPANDED_HEIGHT
                } else {
                    COLLAPSED_HEIGHT
                },
            );
        }
        if on {
            self.quick_add.focus();
        }
    }
}

use gtk::glib;
