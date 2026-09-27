// Project Banshee — queue-first GNOME player.
// SPDX-License-Identifier: Apache-2.0

mod app;
mod ui;

use adw::prelude::*;
use banshee::paths::APP_ID;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> glib::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("banshee=info"))
        .format_timestamp_millis()
        .init();
    // Background panics are converted to errors by runtime::run; log any that reach here.
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));

    // The name GNOME Shell, sound settings and PipeWire show for this app.
    glib::set_application_name("Banshee");
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    let state: Rc<RefCell<Option<Rc<app::Controller>>>> = Rc::default();

    {
        let state = state.clone();
        app.connect_startup(move |app| {
            ui::load_css();
            gtk::Window::set_default_icon_name(APP_ID);
            match app::Controller::new(app) {
                Ok(c) => *state.borrow_mut() = Some(c),
                Err(e) => {
                    log::error!("{e}");
                    let dlg = adw::AlertDialog::new(Some("Banshee Can’t Start"), Some(&e));
                    dlg.add_response("close", "_Close");
                    let win = adw::ApplicationWindow::builder()
                        .application(app)
                        .title("Banshee")
                        .build();
                    let a = app.clone();
                    dlg.connect_response(None, move |_, _| a.quit());
                    win.present();
                    dlg.present(Some(&win));
                }
            }
            install_app_actions(app);
        });
    }
    {
        let state = state.clone();
        app.connect_activate(move |app| {
            let Some(ctl) = state.borrow().clone() else {
                return;
            };
            let win = match app.active_window() {
                Some(w) => w,
                None => {
                    let w = ui::window::build(app, &ctl);
                    ctl.announce();
                    w.upcast()
                }
            };
            win.present();
        });
    }
    {
        let state = state.clone();
        app.connect_open(move |app, files, _| {
            app.activate();
            let Some(ctl) = state.borrow().clone() else {
                return;
            };
            for f in files {
                ctl.open_uri(&f.uri());
            }
        });
    }
    {
        let state = state.clone();
        app.connect_shutdown(move |_| {
            if let Some(c) = state.borrow_mut().take() {
                c.shutdown();
            }
        });
    }
    // Logout, `kill` and Ctrl+C still save the queue and search memory (ADR 0012).
    let (tx, rx) = async_channel::bounded::<&'static str>(1);
    banshee::runtime::runtime().spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
            signal(SignalKind::hangup()),
        ) else {
            log::warn!("cannot install signal handlers; state is still saved continuously");
            return;
        };
        let name = tokio::select! {
            _ = term.recv() => "SIGTERM",
            _ = int.recv() => "SIGINT",
            _ = hup.recv() => "SIGHUP",
        };
        let _ = tx.send(name).await;
    });
    {
        let a = app.clone();
        glib::spawn_future_local(async move {
            if let Ok(name) = rx.recv().await {
                log::info!("{name} received, saving and quitting");
                a.quit();
            }
        });
    }
    app.run()
}

fn install_app_actions(app: &adw::Application) {
    let quit = gio::SimpleAction::new("quit", None);
    {
        let a = app.clone();
        quit.connect_activate(move |_, _| a.quit());
    }
    app.add_action(&quit);

    let about = gio::SimpleAction::new("about", None);
    {
        let a = app.clone();
        about.connect_activate(move |_, _| {
            let d = adw::AboutDialog::builder()
                .application_name("Banshee")
                .application_icon(APP_ID)
                .version(env!("BANSHEE_VERSION"))
                .developer_name("Jorge O. Castro")
                .license_type(gtk::License::Apache20)
                .website("https://github.com/castrojo/projectbanshee")
                .issue_url("https://github.com/castrojo/projectbanshee/issues")
                .comments("A queue-first player for YouTube Music, YouTube and Spotify, in the spirit of the classic Banshee. Not affiliated with the original project.")
                .build();
            d.add_legal_section(
                "Banshee icon",
                Some("Copyright © 2005–2014 Novell, Inc. and contributors"),
                gtk::License::MitX11,
                None,
            );
            d.present(a.active_window().as_ref());
        });
    }
    app.add_action(&about);

    let shortcuts = gio::SimpleAction::new("shortcuts", None);
    {
        let a = app.clone();
        shortcuts.connect_activate(move |_, _| {
            let d = adw::ShortcutsDialog::new();
            let queue = adw::ShortcutsSection::new(Some("Queueing"));
            for (t, k) in [
                ("Search", "<Control>f <Control>l <Alt>1"),
                ("Add highlighted result to queue", "Return"),
                ("Play highlighted result next", "<Shift>Return"),
                ("Play highlighted result now", "<Control>Return"),
                ("Move highlight", "Up Down"),
                ("Show or hide the queue", "F9"),
            ] {
                queue.add(adw::ShortcutsItem::new(t, k));
            }
            d.add(queue);
            let play = adw::ShortcutsSection::new(Some("Playback"));
            for (t, k) in [
                ("Play or pause", "<Control>space"),
                ("Next", "<Control>Right"),
                ("Previous", "<Control>Left"),
                ("Mini Mode", "<Control>m"),
                ("Touch Mode", "F11"),
            ] {
                play.add(adw::ShortcutsItem::new(t, k));
            }
            d.add(play);
            let mini = adw::ShortcutsSection::new(Some("Mini Mode"));
            for (t, k) in [
                ("Search and add to queue", "<Control>f"),
                ("Close search", "Escape"),
                ("Play or pause", "space"),
            ] {
                mini.add(adw::ShortcutsItem::new(t, k));
            }
            d.add(mini);
            let touch = adw::ShortcutsSection::new(Some("Touch Mode"));
            for (t, k) in [
                ("Search and add to queue", "<Control>f"),
                ("Leave Touch Mode", "Escape F11"),
            ] {
                touch.add(adw::ShortcutsItem::new(t, k));
            }
            d.add(touch);
            let general = adw::ShortcutsSection::new(Some("General"));
            for (t, k) in [
                ("Library", "<Alt>2"),
                ("Accounts", "<Control>comma"),
                ("Keyboard shortcuts", "<Control>question"),
                ("Close window", "<Control>w"),
                ("Quit", "<Control>q"),
            ] {
                general.add(adw::ShortcutsItem::new(t, k));
            }
            d.add(general);
            d.present(a.active_window().as_ref());
        });
    }
    app.add_action(&shortcuts);

    for (action, accels) in [
        ("app.quit", &["<Control>q"][..]),
        ("app.shortcuts", &["<Control>question"]),
        ("window.close", &["<Control>w"]),
        ("win.focus-search", &["<Control>f", "<Control>l", "<Alt>1"]),
        ("win.show-library", &["<Alt>2"]),
        ("win.toggle-queue", &["F9"]),
        ("win.play-pause", &["<Control>space"]),
        ("win.next", &["<Control>Right"]),
        ("win.previous", &["<Control>Left"]),
        ("win.mini-mode", &["<Control>m"]),
        ("win.touch-mode", &["F11"]),
        ("win.accounts", &["<Control>comma"]),
    ] {
        app.set_accels_for_action(action, accels);
    }
}
