//! GTK/libadwaita user interface.

pub mod accounts;
pub mod library;
pub mod now_playing;
pub mod queue_panel;
pub mod rows;
pub mod search;
pub mod window;

use crate::app::{Controller, ToastSpec};
use adw::prelude::*;
use banshee::model::{Collection, CollectionKind, SourceKind, Track};
use gtk::glib;
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

/// Share to Discord (ADR 0009).
pub fn share_to_discord(ctl: &Rc<Controller>, win: &impl IsA<gtk::Window>, tracks: &[Track]) {
    let msg = banshee::discord::share_message(tracks);
    let Some(display) = gtk::gdk::Display::default() else {
        ctl.toast_error("No display available for the clipboard");
        return;
    };
    let (ctl, win) = (ctl.clone(), win.clone().upcast::<gtk::Window>());
    glib::spawn_future_local(async move {
        match banshee::discord::share(&display, Some(&win), &msg).await {
            Ok(()) => ctl.toast(ToastSpec::info("Links copied — paste them in Discord")),
            Err(e) => ctl.toast_error(e),
        }
    });
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
