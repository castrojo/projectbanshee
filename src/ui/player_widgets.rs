//! Building blocks shared by the Now Playing Bar and Mini Mode: track info, transport,
//! progress, extras (shuffle/repeat/volume) and the artwork-derived accent.

use crate::app::{AppEvent, Controller};
use crate::ui::rows::{Artwork, album_line, subtitle_for};
use adw::prelude::*;
use banshee::model::{MediaKind, format_duration};
use banshee::player::PlaybackState;
use banshee::queue::{Advance, RepeatMode};
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub fn icon_button(icon: &str, tip: &str) -> gtk::Button {
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

fn is_active(s: PlaybackState) -> bool {
    matches!(
        s,
        PlaybackState::Playing | PlaybackState::Buffering(_) | PlaybackState::Loading
    )
}

/// Artwork (click → artist on YouTube Music), title, artist line and "Up next".
pub struct TrackInfo {
    pub root: gtk::Box,
    pub backdrop: gtk::Picture,
    pub art: Artwork,
    /// Title, artist, album and "Up next" labels.
    text: gtk::Box,
}

impl TrackInfo {
    /// Cover above centred text (Touch Mode's stage), instead of cover beside text.
    pub fn stack_vertically(&self, spacing: i32) {
        self.root.set_orientation(gtk::Orientation::Vertical);
        self.root.set_spacing(spacing);
        let mut child = self.root.first_child();
        while let Some(w) = child {
            w.set_halign(gtk::Align::Center);
            child = w.next_sibling();
        }
        let mut child = self.text.first_child();
        while let Some(w) = child {
            if let Some(label) = w.downcast_ref::<gtk::Label>() {
                label.set_xalign(0.5);
                label.set_justify(gtk::Justification::Center);
                label.set_wrap(true);
                label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                label.set_lines(2);
            }
            child = w.next_sibling();
        }
    }
}

impl TrackInfo {
    /// `terse`: artist only (Mini Mode), instead of kind · artist · album · length.
    pub fn new(ctl: &Rc<Controller>, art_px: i32, show_up_next: bool, terse: bool) -> Self {
        let art = Artwork::new(art_px);
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

        let label = |classes: &[&str]| {
            let l = gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            for c in classes {
                l.add_css_class(c);
            }
            l
        };
        let title = label(&["track-title"]);
        title.set_label("Nothing playing");
        let artist = label(&["track-artist"]);
        artist.set_label("Search and press Enter to start a queue");
        let album = label(&["track-album", "caption"]);
        album.set_visible(false);
        let up_next = label(&["up-next", "caption"]);
        up_next.set_visible(false);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_valign(gtk::Align::Center);
        text.set_hexpand(true);
        text.set_width_request(96);
        text.append(&title);
        text.append(&artist);
        text.append(&album);
        if show_up_next {
            text.append(&up_next);
        }

        let root = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        root.append(&art_btn);
        root.append(&text);

        // Blurred copy of the artwork for backdrops (used by Mini Mode).
        let backdrop = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .hexpand(true)
            .vexpand(true)
            .build();
        backdrop.add_css_class("artwork-backdrop");

        {
            let ctl = ctl.clone();
            art_btn.connect_clicked(move |_| {
                if let Some(e) = ctl.current_entry() {
                    crate::ui::open_uri(&ctl, &e.track.artist_page_url());
                }
            });
        }

        let video: Rc<RefCell<Option<gdk::Paintable>>> = Rc::default();
        let weak = Rc::downgrade(ctl);
        let (art2, backdrop2) = (art.clone(), backdrop.clone());
        ctl.subscribe(move |ev| {
            let Some(ctl) = weak.upgrade() else { return };
            match ev {
                AppEvent::NowPlaying(entry) => match entry {
                    Some(e) => {
                        title.set_label(&e.track.title);
                        title.set_tooltip_text(Some(&e.track.title));
                        let al = album_line(&e.track);
                        album.set_label(al.as_deref().unwrap_or_default());
                        album.set_visible(al.is_some());
                        artist.set_label(&if terse {
                            e.track.artist.clone()
                        } else {
                            subtitle_for(&e.track)
                        });
                        art_btn.set_sensitive(true);
                        art2.set_icon(e.track.kind.icon_name());
                        let live = video
                            .borrow()
                            .clone()
                            .filter(|_| e.track.kind == MediaKind::Video);
                        match live {
                            Some(p) => art2.set_paintable(Some(&p)),
                            None => art2.load(&ctl, e.track.thumbnail_url.as_deref()),
                        }
                        load_backdrop(&ctl, &backdrop2, e.track.thumbnail_url.as_deref());
                    }
                    None => {
                        title.set_label("Nothing playing");
                        album.set_visible(false);
                        artist.set_label("Search and press Enter to start a queue");
                        art_btn.set_sensitive(false);
                        art2.load(&ctl, None);
                        backdrop2.set_paintable(gdk::Paintable::NONE);
                    }
                },
                AppEvent::Video(p) => {
                    *video.borrow_mut() = p.clone();
                    match (p, ctl.current_entry()) {
                        (Some(p), Some(e)) if e.track.kind == MediaKind::Video => {
                            art2.set_paintable(Some(p))
                        }
                        (_, Some(e)) => art2.load(&ctl, e.track.thumbnail_url.as_deref()),
                        _ => {}
                    }
                }
                AppEvent::State(s) => {
                    let loading = matches!(s, PlaybackState::Loading | PlaybackState::Buffering(_));
                    spinner.set_visible(loading);
                    let tip = match s {
                        PlaybackState::Loading => "Loading stream…".to_string(),
                        PlaybackState::Buffering(p) => format!("Buffering {p}%"),
                        _ => "Open Artist on YouTube Music".to_string(),
                    };
                    art_btn.set_tooltip_text(Some(&tip));
                }
                _ => {}
            }
            if matches!(
                ev,
                AppEvent::NowPlaying(_) | AppEvent::QueueChanged | AppEvent::ModesChanged
            ) {
                match ctl.up_next() {
                    Some(n) => {
                        let more = ctl
                            .queue_len()
                            .saturating_sub(ctl.current_index().map_or(0, |i| i + 2));
                        let tail = if more > 0 {
                            format!(" · {more} more")
                        } else {
                            String::new()
                        };
                        up_next.set_label(&format!(
                            "Up next: {} — {}{tail}",
                            n.track.title, n.track.artist
                        ));
                        up_next.set_visible(show_up_next);
                    }
                    None => up_next.set_visible(false),
                }
            }
        });
        Self {
            root,
            backdrop,
            art,
            text,
        }
    }
}

fn load_backdrop(ctl: &Rc<Controller>, pic: &gtk::Picture, url: Option<&str>) {
    let Some(url) = url.map(|u| banshee::artwork::sized_thumbnail(u, 120)) else {
        pic.set_paintable(gdk::Paintable::NONE);
        return;
    };
    let (store, pic, ctl) = (ctl.artwork.clone(), pic.clone(), ctl.clone());
    glib::spawn_future_local(async move {
        if let Ok(tex) = store.load(&url).await {
            pic.set_paintable(Some(&tex));
            crate::ui::set_accent(banshee::artwork::dominant_color(&tex));
            let _ = &ctl;
        }
    });
}

/// Previous / Play-Pause / Next as one centred cluster.
pub struct Transport {
    pub root: gtk::Box,
}

impl Transport {
    pub fn new(ctl: &Rc<Controller>, play_px: i32) -> Self {
        let prev = icon_button("media-skip-backward-symbolic", "Previous");
        let next = icon_button("media-skip-forward-symbolic", "Next");
        let play = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .tooltip_text("Play")
            .valign(gtk::Align::Center)
            .width_request(play_px)
            .height_request(play_px)
            .build();
        play.add_css_class("circular");
        play.add_css_class("suggested-action");
        play.add_css_class("play-button");
        play.update_property(&[gtk::accessible::Property::Label("Play")]);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.set_halign(gtk::Align::Center);
        root.set_valign(gtk::Align::Center);
        root.append(&prev);
        root.append(&play);
        root.append(&next);
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
        let weak = Rc::downgrade(ctl);
        ctl.subscribe(move |ev| {
            let Some(ctl) = weak.upgrade() else { return };
            match ev {
                AppEvent::State(s) => {
                    let (icon, tip) = if is_active(*s) {
                        ("media-playback-pause-symbolic", "Pause")
                    } else {
                        ("media-playback-start-symbolic", "Play")
                    };
                    play.set_icon_name(icon);
                    play.set_tooltip_text(Some(tip));
                    play.update_property(&[gtk::accessible::Property::Label(tip)]);
                }
                AppEvent::QueueChanged | AppEvent::ModesChanged => {
                    let has = ctl.queue_len() > 0;
                    prev.set_sensitive(has);
                    next.set_sensitive(has);
                    play.set_sensitive(has);
                }
                _ => {}
            }
        });
        Self { root }
    }
}

/// Seek bar with elapsed/total labels (or bare, for Mini Mode).
pub struct Progress {
    pub root: gtk::Box,
}

impl Progress {
    pub fn new(ctl: &Rc<Controller>, with_labels: bool) -> Self {
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
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        if with_labels {
            root.append(&pos_label);
        }
        root.append(&scale);
        if with_labels {
            root.append(&dur_label);
        } else {
            scale.add_css_class("thin-progress");
        }

        let hold_until: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        {
            let (ctl, hold, pending, pos_label, scale2) = (
                ctl.clone(),
                hold_until.clone(),
                pending.clone(),
                pos_label.clone(),
                scale.clone(),
            );
            scale.connect_change_value(move |_, _, v| {
                hold.set(Some(Instant::now() + Duration::from_millis(700)));
                let text = format_duration(v.max(0.0) as u64);
                pos_label.set_label(&text);
                scale2.set_tooltip_text(Some(&text));
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
        ctl.subscribe(move |ev| match ev {
            AppEvent::NowPlaying(entry) => {
                let d = entry
                    .as_ref()
                    .and_then(|e| e.track.duration_secs)
                    .unwrap_or(0) as f64;
                scale.set_range(0.0, d.max(1.0));
                scale.set_value(0.0);
                scale.set_sensitive(entry.is_some() && d > 0.0);
                pos_label.set_label("0:00");
                dur_label.set_label(
                    &entry
                        .as_ref()
                        .map(|e| e.track.duration_label())
                        .unwrap_or_default(),
                );
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
                let text = format_duration(position.as_secs());
                pos_label.set_label(&text);
                scale.set_tooltip_text(Some(&text));
            }
            _ => {}
        });
        Self { root }
    }
}

fn repeat_icon(m: RepeatMode) -> (&'static str, &'static str) {
    match m {
        RepeatMode::Off => ("media-playlist-consecutive-symbolic", "Repeat: Off"),
        RepeatMode::All => ("media-playlist-repeat-symbolic", "Repeat: All"),
        RepeatMode::One => ("media-playlist-repeat-song-symbolic", "Repeat: One"),
    }
}

/// Shuffle, repeat and volume.
pub struct Extras {
    pub root: gtk::Box,
}

impl Extras {
    pub fn new(ctl: &Rc<Controller>) -> Self {
        let shuffle = gtk::ToggleButton::builder()
            .icon_name("media-playlist-shuffle-symbolic")
            .tooltip_text("Shuffle")
            .valign(gtk::Align::Center)
            .build();
        shuffle.add_css_class("flat");
        shuffle.add_css_class("circular");
        shuffle.update_property(&[gtk::accessible::Property::Label("Shuffle")]);
        let (ri, rt) = repeat_icon(RepeatMode::Off);
        let repeat = icon_button(ri, rt);
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
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        root.set_valign(gtk::Align::Center);
        root.append(&shuffle);
        root.append(&repeat);
        root.append(&volume);

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
        let weak = Rc::downgrade(ctl);
        ctl.subscribe(move |ev| {
            let Some(ctl) = weak.upgrade() else { return };
            if let AppEvent::ModesChanged | AppEvent::QueueChanged = ev {
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
            }
        });
        Self { root }
    }
}
