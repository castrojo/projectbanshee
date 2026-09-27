//! Accounts dialog: YouTube Music session import and Spotify browser sign-in.

use crate::app::{Controller, ToastSpec};
use adw::prelude::*;
use banshee::model::SourceKind;
use banshee::runtime::run;
use banshee::sources::{AudioSource, SourceError, cookies};
use gtk::{gio, glib};
use std::rc::Rc;

pub fn present(ctl: &Rc<Controller>, parent: &impl IsA<gtk::Widget>) {
    let dialog = adw::PreferencesDialog::builder()
        .title("Accounts")
        .search_enabled(false)
        .build();
    let page = adw::PreferencesPage::builder()
        .title("Accounts")
        .icon_name("system-users-symbolic")
        .build();
    dialog.add(&page);

    // ---------------- YouTube Music
    let yt = adw::PreferencesGroup::builder()
        .title("YouTube Music")
        .description(
            "Sign in with your browser, close it, then import its session. Only Google and YouTube cookies are kept, in a private file.",
        )
        .build();
    let status = adw::ActionRow::builder().title("Status").build();
    let open = adw::ButtonRow::builder()
        .title("Open YouTube Music in Browser")
        .end_icon_name("adw-external-link-symbolic")
        .build();
    yt.add(&status);
    yt.add(&open);
    let browsers = cookies::detect_browsers();
    let mut import_rows = Vec::new();
    if browsers.is_empty() {
        yt.add(
            &adw::ActionRow::builder()
                .title("No Flatpak browser found")
                .subtitle("Firefox, Chrome and Brave Flatpaks are supported. You can also import a cookies.txt file.")
                .build(),
        );
    }
    for b in browsers {
        let row = adw::ActionRow::builder()
            .title(format!("Import from {}", b.label))
            .activatable(true)
            .build();
        let spinner = adw::Spinner::builder().visible(false).build();
        row.add_suffix(&spinner);
        row.add_suffix(&gtk::Image::from_icon_name("folder-download-symbolic"));
        yt.add(&row);
        import_rows.push((row, spinner, b.spec));
    }
    let file_row = adw::ActionRow::builder()
        .title("Import cookies.txt…")
        .activatable(true)
        .build();
    file_row.add_suffix(&gtk::Image::from_icon_name("document-open-symbolic"));
    yt.add(&file_row);
    let yt_out = adw::ButtonRow::builder()
        .title("Sign Out of YouTube Music")
        .build();
    yt_out.add_css_class("destructive-action");
    yt.add(&yt_out);
    page.add(&yt);

    // ---------------- Spotify
    let sp = adw::PreferencesGroup::builder()
        .title("Spotify")
        .description("Sign in once in your browser. Playback requires Spotify Premium.")
        .build();
    let sp_status = adw::ActionRow::builder().title("Status").build();
    let sp_in = adw::ButtonRow::builder()
        .title("Sign In with Browser")
        .end_icon_name("adw-external-link-symbolic")
        .build();
    sp_in.add_css_class("suggested-action");
    let sp_cancel = adw::ButtonRow::builder()
        .title("Cancel Sign-In")
        .visible(false)
        .build();
    let sp_out = adw::ButtonRow::builder()
        .title("Sign Out of Spotify")
        .build();
    sp_out.add_css_class("destructive-action");
    sp.add(&sp_status);
    sp.add(&sp_in);
    sp.add(&sp_cancel);
    sp.add(&sp_out);
    page.add(&sp);

    // ---------------- Discord
    let dc = adw::PreferencesGroup::builder()
        .title("Discord")
        .description("Show the song, artist and artwork you're playing as your Discord status.")
        .build();
    let dc_on = adw::SwitchRow::builder()
        .title("Show What I'm Playing")
        .active(ctl.prefs.borrow().discord_presence)
        .build();
    let dc_id = adw::EntryRow::builder()
        .title("Discord Application ID")
        .text(
            ctl.prefs
                .borrow()
                .discord_client_id
                .clone()
                .unwrap_or_default(),
        )
        .show_apply_button(true)
        .input_purpose(gtk::InputPurpose::Digits)
        .build();
    let dc_help = adw::ActionRow::builder()
        .title("Create an Application ID")
        .subtitle("Discord only shows statuses from registered applications. Create one named “Project Banshee” and paste its Application ID above.")
        .activatable(true)
        .build();
    dc_help.add_suffix(&gtk::Image::from_icon_name("adw-external-link-symbolic"));
    let dc_status = adw::ActionRow::builder().title("Status").build();
    dc.add(&dc_on);
    dc.add(&dc_id);
    dc.add(&dc_status);
    dc.add(&dc_help);
    page.add(&dc);
    {
        let ctl = ctl.clone();
        dc_help.connect_activated(move |_| {
            crate::ui::open_uri(&ctl, "https://discord.com/developers/applications")
        });
    }
    {
        let (ctl, dc_id) = (ctl.clone(), dc_id.clone());
        dc_on.connect_active_notify(move |r| {
            ctl.set_discord(r.is_active(), Some(dc_id.text().to_string()))
        });
    }
    {
        let (ctl, dc_on) = (ctl.clone(), dc_on.clone());
        dc_id
            .connect_apply(move |e| ctl.set_discord(dc_on.is_active(), Some(e.text().to_string())));
    }
    {
        use banshee::discord::PresenceStatus as S;
        let describe = |s: &S| match s {
            S::Off => "Off".to_string(),
            S::NoClientId => "Add an Application ID to turn this on".to_string(),
            S::DiscordNotRunning => "Waiting for Discord to start".to_string(),
            S::Connecting => "Connecting to Discord…".to_string(),
            S::Connected { user } => format!("Showing on Discord as {user}"),
            S::Rejected(why) => format!("Discord refused: {why}"),
        };
        dc_status.set_subtitle(&glib::markup_escape_text(&describe(&ctl.presence.status())));
        let rx = ctl.presence.subscribe();
        let row = dc_status.downgrade();
        glib::spawn_future_local(async move {
            while let Ok(s) = rx.recv().await {
                let Some(row) = row.upgrade() else { break };
                row.set_subtitle(&glib::markup_escape_text(&describe(&s)));
            }
        });
    }

    let refresh: Rc<dyn Fn()> = {
        let (ctl, status, yt_out, sp_status, sp_in, sp_out) = (
            ctl.clone(),
            status.clone(),
            yt_out.clone(),
            sp_status.clone(),
            sp_in.clone(),
            sp_out.clone(),
        );
        Rc::new(move || {
            let yt_on = ctl.youtube.is_signed_in();
            status.set_subtitle(if yt_on {
                "Session imported — library available"
            } else {
                "Not signed in — search and playback still work"
            });
            yt_out.set_visible(yt_on);
            let sp_on = ctl.spotify.is_signed_in();
            let name = ctl.spotify.display_name();
            sp_status.set_subtitle(&match (sp_on, name) {
                (true, Some(n)) => format!("Signed in as {}", glib::markup_escape_text(&n)),
                (true, None) => "Signed in".into(),
                (false, _) => "Not signed in".into(),
            });
            sp_in.set_visible(!sp_on);
            sp_out.set_visible(sp_on);
        })
    };
    refresh();

    {
        let ctl = ctl.clone();
        open.connect_activated(move |_| crate::ui::open_uri(&ctl, "https://music.youtube.com"));
    }

    let after_import: Rc<dyn Fn(Result<(), SourceError>)> = {
        let (ctl, refresh) = (ctl.clone(), refresh.clone());
        Rc::new(move |r| {
            let (ctl, refresh) = (ctl.clone(), refresh.clone());
            match r {
                Ok(()) => {
                    glib::spawn_future_local(async move {
                        match run(ctl.youtube.reload_auth())
                            .await
                            .map_err(SourceError::Unavailable)
                            .and_then(|r| r)
                        {
                            Ok(true) => {
                                ctl.purge_source_cache(SourceKind::YouTubeMusic);
                                ctl.toast(ToastSpec::info("YouTube Music session imported"));
                            }
                            Ok(false) => ctl.toast_error(
                                "The imported session was not accepted by YouTube Music",
                            ),
                            Err(e) => {
                                ctl.toast_error(crate::app::describe(SourceKind::YouTubeMusic, &e))
                            }
                        }
                        ctl.accounts_changed();
                        refresh();
                    });
                }
                Err(e) => ctl.toast_error(format!("Import failed — {e}")),
            }
        })
    };

    for (row, spinner, spec) in import_rows {
        let after = after_import.clone();
        row.connect_activated(move |row| {
            row.set_sensitive(false);
            spinner.set_visible(true);
            let (row, spinner, spec, after) =
                (row.clone(), spinner.clone(), spec.clone(), after.clone());
            glib::spawn_future_local(async move {
                let r = run(async move { cookies::import_from_browser(&spec).await })
                    .await
                    .map_err(SourceError::Unavailable)
                    .and_then(|r| r);
                row.set_sensitive(true);
                spinner.set_visible(false);
                after(r);
            });
        });
    }
    {
        let (after, dialog_w) = (after_import.clone(), dialog.downgrade());
        file_row.connect_activated(move |_| {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Netscape cookie files"));
            filter.add_pattern("*.txt");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let fd = gtk::FileDialog::builder()
                .title("Import cookies.txt")
                .filters(&filters)
                .modal(true)
                .build();
            let root = dialog_w
                .upgrade()
                .and_then(|d| d.root())
                .and_downcast::<gtk::Window>();
            let after = after.clone();
            glib::spawn_future_local(async move {
                let file = match fd.open_future(root.as_ref()).await {
                    Ok(f) => f,
                    Err(e)
                        if e.matches(gtk::DialogError::Dismissed)
                            || e.matches(gtk::DialogError::Cancelled) =>
                    {
                        return;
                    }
                    Err(e) => {
                        after(Err(SourceError::Unavailable(e.to_string())));
                        return;
                    }
                };
                let Some(path) = file.path() else {
                    after(Err(SourceError::Unavailable(
                        "the selected file is not local".into(),
                    )));
                    return;
                };
                let r = run(async move { cookies::import_from_file(&path).await })
                    .await
                    .map_err(SourceError::Unavailable)
                    .and_then(|r| r);
                after(r);
            });
        });
    }
    {
        let (ctl, refresh) = (ctl.clone(), refresh.clone());
        yt_out.connect_activated(move |_| {
            if let Err(e) = cookies::sign_out() {
                ctl.toast_error(format!("Couldn’t remove the YouTube session: {e}"));
            }
            let (ctl, refresh) = (ctl.clone(), refresh.clone());
            glib::spawn_future_local(async move {
                let _ = run(ctl.youtube.reload_auth()).await;
                ctl.purge_source_cache(SourceKind::YouTubeMusic);
                ctl.accounts_changed();
                refresh();
            });
        });
    }
    {
        let (ctl, refresh, sp_cancel2) = (ctl.clone(), refresh.clone(), sp_cancel.clone());
        sp_in.connect_activated(move |row| {
            row.set_sensitive(false);
            sp_cancel2.set_visible(true);
            let (ctl, refresh, row, cancel) = (
                ctl.clone(),
                refresh.clone(),
                row.clone(),
                sp_cancel2.clone(),
            );
            glib::spawn_future_local(async move {
                let busy = ctl.busy_guard();
                let r = run(ctl.spotify.sign_in())
                    .await
                    .map_err(SourceError::Unavailable)
                    .and_then(|r| r);
                drop(busy);
                row.set_sensitive(true);
                cancel.set_visible(false);
                match r {
                    Ok(name) => {
                        ctl.purge_source_cache(SourceKind::Spotify);
                        ctl.toast(ToastSpec::info(format!("Signed in to Spotify as {name}")));
                    }
                    Err(e) => ctl.toast_error(crate::app::describe(SourceKind::Spotify, &e)),
                }
                ctl.accounts_changed();
                refresh();
            });
        });
    }
    {
        let ctl = ctl.clone();
        sp_cancel.connect_activated(move |_| ctl.spotify.cancel_sign_in());
    }
    {
        let (ctl, refresh) = (ctl.clone(), refresh.clone());
        sp_out.connect_activated(move |_| {
            ctl.spotify.sign_out();
            ctl.purge_source_cache(SourceKind::Spotify);
            ctl.accounts_changed();
            refresh();
        });
    }
    {
        let ctl = ctl.clone();
        dialog.connect_closed(move |_| ctl.spotify.cancel_sign_in());
    }
    dialog.present(Some(parent));
}
