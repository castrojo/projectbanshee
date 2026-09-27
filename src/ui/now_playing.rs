//! Now Playing Bar (full window): track on the left, transport and seek bar centred on
//! the window, shuffle/repeat/volume and the menu on the right.

use crate::app::Controller;
use crate::ui::player_widgets::{Extras, Progress, TrackInfo, Transport};
use adw::prelude::*;
use gtk::gio;
use std::rc::Rc;

pub struct NowPlaying {
    pub root: gtk::CenterBox,
    /// Widgets hidden when the window is narrow.
    pub compact_widgets: Vec<gtk::Widget>,
    /// Widgets whose fixed width only applies when the window is wide.
    pub wide_widths: Vec<gtk::Widget>,
}

impl NowPlaying {
    pub fn new(ctl: &Rc<Controller>) -> Rc<Self> {
        let info = TrackInfo::new(ctl, 56, true, false);
        info.root.set_width_request(220);
        // Keep the track text clear of the centred transport.
        info.root.set_margin_end(18);

        let transport = Transport::new(ctl, 44);
        let progress = Progress::new(ctl, true);
        progress.root.set_width_request(280);
        let center = gtk::Box::new(gtk::Orientation::Vertical, 2);
        center.set_valign(gtk::Align::Center);
        center.append(&transport.root);
        center.append(&progress.root);

        let extras = Extras::new(ctl);
        let menu = gio::Menu::new();
        menu.append(
            Some("Open Artist on YouTube Music"),
            Some("win.open-artist"),
        );
        menu.append(Some("Mini Mode"), Some("win.mini-mode"));
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("Now Playing Menu")
            .menu_model(&menu)
            .valign(gtk::Align::Center)
            .build();
        menu_btn.add_css_class("flat");
        menu_btn.add_css_class("circular");
        let mini_btn = gtk::Button::builder()
            .icon_name("view-restore-symbolic")
            .tooltip_text("Mini Mode")
            .action_name("win.mini-mode")
            .valign(gtk::Align::Center)
            .build();
        mini_btn.add_css_class("flat");
        mini_btn.add_css_class("circular");
        mini_btn.update_property(&[gtk::accessible::Property::Label("Mini Mode")]);
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        end.set_halign(gtk::Align::End);
        end.set_margin_start(18);
        end.append(&extras.root);
        end.append(&mini_btn);
        end.append(&menu_btn);

        let root = gtk::CenterBox::builder()
            .start_widget(&info.root)
            .center_widget(&center)
            .end_widget(&end)
            .build();
        root.add_css_class("now-playing-bar");
        root.add_css_class("banshee-accent");

        Rc::new(Self {
            root,
            compact_widgets: vec![extras.root.clone().upcast()],
            wide_widths: vec![info.root.clone().upcast(), progress.root.clone().upcast()],
        })
    }
}
