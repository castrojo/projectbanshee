//! GTK/libadwaita user interface.

pub mod accounts;
pub mod home;
pub mod library;
pub mod mini;
pub mod now_playing;
pub mod player_widgets;
pub mod queue_panel;
pub mod rows;
pub mod search;
pub mod touch;
pub mod window;

use crate::app::Controller;
use adw::prelude::*;
use banshee::model::{Collection, CollectionKind, SourceKind};
use gtk::{gio, glib};
use std::rc::Rc;

/// Open a web link in the user's browser via the OpenURI portal.
pub fn open_uri(ctl: &Rc<Controller>, uri: &str) {
    let win = ctl.app.active_window();
    let (ctl, uri) = (ctl.clone(), uri.to_string());
    glib::spawn_future_local(async move {
        if let Err(e) = gtk::UriLauncher::new(&uri)
            .launch_future(win.as_ref())
            .await
        {
            ctl.toast_error(format!("Couldn’t open {uri}: {e}"));
        }
    });
}

pub fn collection_url(c: &Collection) -> String {
    match (c.source, c.kind) {
        (SourceKind::YouTubeMusic, CollectionKind::Playlist | CollectionKind::LikedSongs) => {
            format!("https://music.youtube.com/playlist?list={}", c.id)
        }
        (SourceKind::YouTubeMusic, CollectionKind::Artist) => {
            format!("https://music.youtube.com/channel/{}", c.id)
        }
        (SourceKind::YouTubeMusic, _) => format!("https://music.youtube.com/browse/{}", c.id),
        (SourceKind::Spotify, CollectionKind::Playlist) => {
            format!("https://open.spotify.com/playlist/{}", c.id)
        }
        (SourceKind::Spotify, CollectionKind::Album) => {
            format!("https://open.spotify.com/album/{}", c.id)
        }
        (SourceKind::Spotify, CollectionKind::Artist) => {
            format!("https://open.spotify.com/artist/{}", c.id)
        }
        (SourceKind::Spotify, CollectionKind::Podcast) => {
            format!("https://open.spotify.com/show/{}", c.id)
        }
        (SourceKind::Spotify, CollectionKind::LikedSongs) => {
            "https://open.spotify.com/collection/tracks".into()
        }
    }
}

pub fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

thread_local! {
    static ACCENT: gtk::CssProvider = {
        let p = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &p, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        }
        p
    };
}

/// Removing the row that holds a list's focus (the one just tapped or clicked) sends a
/// GtkListView back to the top. Call this before creating the list's selection model over
/// `store`, so it sees a removal first: it notes the position and puts it back once the list
/// has laid out.
pub fn hold_scroll_on_removal(store: &gio::ListStore, scroller: &gtk::ScrolledWindow) {
    let weak = scroller.downgrade();
    store.connect_items_changed(move |_, _, removed, added| {
        let Some(scroller) = weak.upgrade() else {
            return;
        };
        if removed == 0 || added > 0 || !scroller.is_mapped() {
            return;
        }
        let adj = scroller.vadjustment();
        let value = adj.value();
        let frames = std::cell::Cell::new(0);
        scroller.add_tick_callback(move |_, _| {
            let max = (adj.upper() - adj.page_size()).max(adj.lower());
            adj.set_value(value.min(max));
            frames.set(frames.get() + 1);
            // A few frames: the list's reset can land several frames after the removal.
            if frames.get() < 8 {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    });
}

/// Tint the play button and Mini Mode with a colour taken from the current artwork.
pub fn set_accent((r, g, b): (u8, u8, u8)) {
    ACCENT.with(|p| {
        p.load_from_string(&format!(
            ".banshee-accent {{ --accent-bg-color: rgb({r},{g},{b}); --banshee-art: rgb({r},{g},{b}); }}"
        ))
    });
}
