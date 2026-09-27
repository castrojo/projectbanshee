//! Now Playing Bar: artwork (click → artist on YouTube Music), metadata, transport, seek,
//! shuffle/repeat, volume.

use crate::app::{AppEvent, Controller};
use crate::ui::rows::{Artwork, subtitle_for};
use adw::prelude::*;
use banshee::model::{MediaKind, format_duration};
use banshee::player::PlaybackState;
use banshee::queue::{Advance, RepeatMode};
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub struct NowPlaying {
    pub root: gtk::Box,
    pub compact_widgets: Vec<gtk::Widget>,
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tip)
        .valign(gtk::Align::Center)
        .build();
    b.add_css_class("flat");
    b.add_css_class("circular");
    b.update_property(&[gtk::accessible::Property::Label(tip)]);
    b
}

fn repeat_icon(m: RepeatMode) -> (&'static str, &'static str) {
    match m {
        RepeatMode::Off => ("media-playlist-consecutive-symbolic", "Repeat: Off"),
        RepeatMode::All => ("media-playlist-repeat-symbolic", "Repeat: All"),
        RepeatMode::One => ("media-playlist-repeat-song-symbolic", "Repeat: One"),
    }
}

impl NowPlaying {
    pub fn new(ctl: &Rc<Controller>) -> Rc<Self> {
        // Artwork button.
        let art = Artwork::new(56);
        let spinner = adw::Spinner::builder()
            .visible(false)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        spinner.set_size_request(24, 24);
        art.root.add_overlay(&spinner);
        let art_btn = gtk::Button::builder()
            .child(&art.root)
            .tooltip_text("Open Artist on YouTube Music")
            .valign(gtk::Align::Center)
            .sensitive(false)
            .build();
        art_btn.add_css_class("flat");
        art_btn.add_css_class("artwork-button");
        art_btn.update_property(&[gtk::accessible::Property::Label(
            "Open artist on YouTube Music",
        )]);

        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .label("Nothing Playing")
            .build();
        title.add_css_class("heading");
        let artist = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .label("Search and queue something")
            .build();
        artist.add_css_class("dim-label");
        artist.add_css_class("caption");
        let meta = gtk::Box::new(gtk::Orientation::Vertical, 2);
        meta.set_valign(gtk::Align::Center);
        meta.set_hexpand(true);
        meta.set_width_request(120);
        meta.append(&title);
        meta.append(&artist);

        let prev = icon_button("media-skip-backward-symbolic", "Previous");
        let play = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .tooltip_text("Play")
            .valign(gtk::Align::Center)
            .build();
        play.add_css_class("circular");
        play.add_css_class("suggested-action");
        play.add_css_class("play-button");
        play.update_property(&[gtk::accessible::Property::Label("Play")]);
        let next = icon_button("media-skip-forward-symbolic", "Next");
        let transport = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        transport.set_halign(gtk::Align::Center);
        transport.append(&prev);
        transport.append(&play);
        transport.append(&next);

        let pos_label = gtk::Label::new(Some("0:00"));
        pos_label.add_css_class("numeric");
        pos_label.add_css_class("caption");
        let dur_label = gtk::Label::new(Some("0:00"));
        dur_label.add_css_class("numeric");
        dur_label.add_css_class("caption");
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_sensitive(false);
        scale.update_property(&[gtk::accessible::Property::Label("Position")]);
        let progress = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        progress.append(&pos_label);
        progress.append(&scale);
        progress.append(&dur_label);

        let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
        center.set_hexpand(true);
        center.set_valign(gtk::Align::Center);
        center.append(&transport);
        center.append(&progress);

        let shuffle = gtk::ToggleButton::builder()
            .icon_name("media-playlist-shuffle-symbolic")
            .tooltip_text("Shuffle")
            .valign(gtk::Align::Center)
            .build();
        shuffle.add_css_class("flat");
        shuffle.add_css_class("circular");
        shuffle.update_property(&[gtk::accessible::Property::Label("Shuffle")]);
        let repeat = icon_button(
            repeat_icon(RepeatMode::Off).0,
            repeat_icon(RepeatMode::Off).1,
        );
        let volume = gtk::ScaleButton::new(
            0.0,
            1.0,
            0.02,
            &[
                "audio-volume-muted-symbolic",
                "audio-volume-high-symbolic",
                "audio-volume-low-symbolic",
                "audio-volume-medium-symbolic",
            ],
        );
        volume.set_value(ctl.player.volume());
        volume.set_valign(gtk::Align::Center);
        volume.set_tooltip_text(Some("Volume"));
        volume.update_property(&[gtk::accessible::Property::Label("Volume")]);

        let menu = gio::Menu::new();
        menu.append(Some("Share to Discord"), Some("win.share-current"));
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

        let extras = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        extras.append(&shuffle);
        extras.append(&repeat);
        extras.append(&volume);

        let left = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        left.append(&art_btn);
        left.append(&meta);
        left.set_hexpand(true);
        left.set_width_request(200);

        let root = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        root.add_css_class("now-playing-bar");
        root.append(&left);
        root.append(&center);
        root.append(&extras);
        root.append(&menu_btn);

        let this = Rc::new(Self {
            root,
            compact_widgets: vec![progress.clone().upcast(), extras.clone().upcast()],
        });

        // --- behaviour
        {
            let ctl = ctl.clone();
            prev.connect_clicked(move |_| ctl.previous());
        }
        {
            let ctl = ctl.clone();
            next.connect_clicked(move |_| ctl.advance(Advance::User));
        }
        {
            let ctl = ctl.clone();
            play.connect_clicked(move |_| ctl.toggle_play());
        }
        {
            let ctl = ctl.clone();
            art_btn.connect_clicked(move |_| {
                if let Some(e) = ctl.current_entry() {
                    crate::ui::open_uri(&ctl, &e.track.artist_page_url());
                }
            });
        }
        let syncing = Rc::new(Cell::new(false));
        {
            let (ctl, syncing) = (ctl.clone(), syncing.clone());
            shuffle.connect_toggled(move |b| {
                if !syncing.get() {
                    ctl.set_shuffle(b.is_active());
                }
            });
        }
        {
            let ctl = ctl.clone();
            repeat.connect_clicked(move |_| ctl.set_repeat(ctl.repeat().cycle()));
        }
        {
            let (ctl, syncing) = (ctl.clone(), syncing.clone());
            volume.connect_value_changed(move |_, v| {
                if !syncing.get() {
                    ctl.set_volume(v);
                }
            });
        }

        // Seeking: debounce while dragging, ignore position ticks briefly after.
        let hold_until: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let pending_seek: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        {
            let (ctl, hold, pending, pos_label) = (
                ctl.clone(),
                hold_until.clone(),
                pending_seek.clone(),
                pos_label.clone(),
            );
            scale.connect_change_value(move |_, _, v| {
                hold.set(Some(Instant::now() + Duration::from_millis(700)));
                pos_label.set_label(&format_duration(v.max(0.0) as u64));
                if let Some(id) = pending.borrow_mut().take() {
                    id.remove();
                }
                let (ctl, pending2) = (ctl.clone(), pending.clone());
                let id = glib::timeout_add_local_once(Duration::from_millis(120), move || {
                    pending2.borrow_mut().take();
                    ctl.seek(Duration::from_secs_f64(v.max(0.0)));
                });
                *pending.borrow_mut() = Some(id);
                glib::Propagation::Proceed
            });
        }

        let weak_ctl = Rc::downgrade(ctl);
        let video: Rc<RefCell<Option<gdk::Paintable>>> = Rc::default();
        ctl.subscribe(move |ev| {
            let Some(ctl) = weak_ctl.upgrade() else {
                return;
            };
            match ev {
                AppEvent::NowPlaying(entry) => match entry {
                    Some(e) => {
                        title.set_label(&e.track.title);
                        title.set_tooltip_text(Some(&e.track.title));
                        artist.set_label(&subtitle_for(&e.track));
                        art_btn.set_sensitive(true);
                        art.set_icon(e.track.kind.icon_name());
                        let live = video
                            .borrow()
                            .clone()
                            .filter(|_| e.track.kind == MediaKind::Video);
                        match live {
                            Some(p) => art.set_paintable(Some(&p)),
                            None => art.load(&ctl, e.track.thumbnail_url.as_deref()),
                        }
                        let d = e.track.duration_secs.unwrap_or(0) as f64;
                        scale.set_range(0.0, d.max(1.0));
                        scale.set_value(0.0);
                        dur_label.set_label(&e.track.duration_label());
                        pos_label.set_label("0:00");
                    }
                    None => {
                        title.set_label("Nothing Playing");
                        artist.set_label("Search and queue something");
                        art_btn.set_sensitive(false);
                        art.load(&ctl, None);
                        scale.set_sensitive(false);
                        scale.set_value(0.0);
                        pos_label.set_label("0:00");
                        dur_label.set_label("0:00");
                    }
                },
                AppEvent::Video(p) => {
                    *video.borrow_mut() = p.clone();
                    let cur = ctl.current_entry();
                    match (p, cur) {
                        (Some(p), Some(e)) if e.track.kind == MediaKind::Video => {
                            art.set_paintable(Some(p))
                        }
                        (_, Some(e)) => art.load(&ctl, e.track.thumbnail_url.as_deref()),
                        _ => {}
                    }
                }
                AppEvent::State(s) => {
                    let playing = matches!(
                        s,
                        PlaybackState::Playing
                            | PlaybackState::Buffering(_)
                            | PlaybackState::Loading
                    );
                    let (icon, tip) = if playing {
                        ("media-playback-pause-symbolic", "Pause")
                    } else {
                        ("media-playback-start-symbolic", "Play")
                    };
                    play.set_icon_name(icon);
                    play.set_tooltip_text(Some(tip));
                    play.update_property(&[gtk::accessible::Property::Label(tip)]);
                    let loading = matches!(s, PlaybackState::Loading | PlaybackState::Buffering(_));
                    spinner.set_visible(loading);
                    let tip = match s {
                        PlaybackState::Loading => "Loading stream…".to_string(),
                        PlaybackState::Buffering(p) => format!("Buffering {p}%"),
                        _ => "Open Artist on YouTube Music".to_string(),
                    };
                    art_btn.set_tooltip_text(Some(&tip));
                }
                AppEvent::Position { position, duration } => {
                    if hold_until.get().is_some_and(|t| Instant::now() < t) {
                        return;
                    }
                    if let Some(d) = duration {
                        scale.set_range(0.0, d.as_secs_f64().max(1.0));
                        dur_label.set_label(&format_duration(d.as_secs()));
                        scale.set_sensitive(true);
                    }
                    scale.set_value(position.as_secs_f64());
                    pos_label.set_label(&format_duration(position.as_secs()));
                }
                AppEvent::ModesChanged | AppEvent::QueueChanged => {
                    syncing.set(true);
                    shuffle.set_active(ctl.is_shuffled());
                    let (icon, tip) = repeat_icon(ctl.repeat());
                    repeat.set_icon_name(icon);
                    repeat.set_tooltip_text(Some(tip));
                    repeat.update_property(&[gtk::accessible::Property::Label(tip)]);
                    if (volume.value() - ctl.player.volume()).abs() > 0.001 {
                        volume.set_value(ctl.player.volume());
                    }
                    syncing.set(false);
                    let has = ctl.queue_len() > 0;
                    prev.set_sensitive(has);
                    next.set_sensitive(has);
                    play.set_sensitive(has);
                }
                _ => {}
            }
        });
        this
    }
}
