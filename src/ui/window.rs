//! Main window: queue-first layout (ADR 0007).

use crate::app::{AppEvent, Controller};
use crate::ui::{
    accounts, library::LibraryView, now_playing::NowPlaying, queue_panel::QueuePanel,
    search::SearchPage,
};
use adw::prelude::*;
use banshee::queue::Advance;
use gtk::{gio, glib};
use std::cell::Cell;
use std::rc::Rc;

const MINI_SIZE: (i32, i32) = (520, 96);

pub fn build(app: &adw::Application, ctl: &Rc<Controller>) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Project Banshee")
        .default_width(ctl.prefs.borrow().window_width.max(360))
        .default_height(ctl.prefs.borrow().window_height.max(200))
        .maximized(ctl.prefs.borrow().maximized)
        .width_request(360)
        .height_request(96)
        .build();

    let search = SearchPage::new(ctl);
    let library = LibraryView::new(ctl);
    let queue = QueuePanel::new(ctl, &window);
    let now = NowPlaying::new(ctl);

    // Video view (visible only while a video plays).
    let video_picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .hexpand(true)
        .vexpand(true)
        .build();
    video_picture.add_css_class("video-view");

    let stack = adw::ViewStack::new();
    let p_search =
        stack.add_titled_with_icon(&search.root, Some("search"), "Search", "edit-find-symbolic");
    let _ = p_search;
    stack.add_titled_with_icon(
        &library.nav,
        Some("library"),
        "Library",
        "folder-music-symbolic",
    );
    let p_video = stack.add_titled_with_icon(
        &video_picture,
        Some("video"),
        "Video",
        "video-x-generic-symbolic",
    );
    p_video.set_visible(false);

    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    let header = adw::HeaderBar::builder().title_widget(&switcher).build();

    let back = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text("Back")
        .visible(false)
        .build();
    back.update_property(&[gtk::accessible::Property::Label("Back")]);
    header.pack_start(&back);
    let busy = adw::Spinner::builder()
        .visible(false)
        .tooltip_text("Loading")
        .build();
    header.pack_start(&busy);

    let main_menu = gio::Menu::new();
    let s1 = gio::Menu::new();
    s1.append(Some("_Accounts"), Some("win.accounts"));
    s1.append(Some("_Mini Mode"), Some("win.mini-mode"));
    main_menu.append_section(None, &s1);
    let s2 = gio::Menu::new();
    s2.append(Some("_Keyboard Shortcuts"), Some("app.shortcuts"));
    s2.append(Some("_About Project Banshee"), Some("app.about"));
    main_menu.append_section(None, &s2);
    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main Menu")
        .menu_model(&main_menu)
        .primary(true)
        .build();
    header.pack_end(&menu_btn);
    let queue_toggle = gtk::ToggleButton::builder()
        .icon_name("view-list-symbolic")
        .tooltip_text("Show Queue")
        .active(ctl.prefs.borrow().show_queue)
        .build();
    queue_toggle.update_property(&[gtk::accessible::Property::Label("Show queue")]);
    header.pack_end(&queue_toggle);

    let switcher_bar = adw::ViewSwitcherBar::builder().stack(&stack).build();
    let content = adw::ToolbarView::builder().content(&stack).build();
    content.add_top_bar(&header);
    content.add_bottom_bar(&switcher_bar);

    let split = adw::OverlaySplitView::builder()
        .content(&content)
        .sidebar(&queue.root)
        .sidebar_position(gtk::PackType::End)
        .min_sidebar_width(300.0)
        .max_sidebar_width(420.0)
        .sidebar_width_fraction(0.32)
        .show_sidebar(true)
        .build();
    queue_toggle
        .bind_property("active", &split, "show-sidebar")
        .bidirectional()
        .sync_create()
        .build();

    // Window buttons live on whichever header is at the window's end edge.
    let sync_buttons = {
        let (split, header, qh) = (split.clone(), header.clone(), queue.header.clone());
        move || {
            let sidebar_at_end = split.shows_sidebar() && !split.is_collapsed();
            header.set_show_end_title_buttons(!sidebar_at_end);
            qh.set_show_end_title_buttons(sidebar_at_end || split.is_collapsed());
            qh.set_show_start_title_buttons(false);
        }
    };
    sync_buttons();
    {
        let s = sync_buttons.clone();
        split.connect_show_sidebar_notify(move |_| s());
        let s = sync_buttons.clone();
        split.connect_collapsed_notify(move |_| s());
    }

    // Mini Mode: exit button + drag handle around the Now Playing Bar.
    let exit_mini = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic")
        .tooltip_text("Leave Mini Mode")
        .valign(gtk::Align::Center)
        .visible(false)
        .action_name("win.mini-mode")
        .build();
    exit_mini.add_css_class("flat");
    exit_mini.add_css_class("circular");
    now.root.append(&exit_mini);
    let bar_handle = gtk::WindowHandle::builder().child(&now.root).build();

    // Toasts float above the content, never over the Now Playing Bar.
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&split));
    let outer = adw::ToolbarView::builder()
        .content(&toasts)
        .bottom_bar_style(adw::ToolbarStyle::Raised)
        .build();
    outer.add_bottom_bar(&bar_handle);
    window.set_content(Some(&outer));

    // Adaptive layout.
    let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        720.0,
        adw::LengthUnit::Sp,
    ));
    bp.add_setter(&split, "collapsed", Some(&true.to_value()));
    bp.add_setter(
        &header,
        "title-widget",
        Some(&None::<gtk::Widget>.to_value()),
    );
    bp.add_setter(&switcher_bar, "reveal", Some(&true.to_value()));
    for w in &now.compact_widgets {
        bp.add_setter(w, "visible", Some(&false.to_value()));
    }
    window.add_breakpoint(bp);
    {
        // Collapsed layout starts with the queue hidden so the search is in focus.
        let (split, toggle) = (split.clone(), queue_toggle.clone());
        split.connect_collapsed_notify(move |s| {
            if s.is_collapsed() {
                toggle.set_active(false);
            }
        });
        let _ = split;
    }

    // Library back navigation from the single header.
    {
        let (nav, back2, stack2) = (library.nav.clone(), back.clone(), stack.clone());
        let sync = move || {
            let deep = nav.navigation_stack().n_items() > 1;
            back2.set_visible(deep && stack2.visible_child_name().as_deref() == Some("library"));
        };
        let s = sync.clone();
        library.nav.connect_visible_page_notify(move |_| s());
        let s = sync.clone();
        stack.connect_visible_child_notify(move |_| s());
        let nav = library.nav.clone();
        back.connect_clicked(move |_| {
            nav.pop();
        });
    }
    {
        let (lib, search2) = (library.clone(), search.clone());
        stack.connect_visible_child_name_notify(move |s| match s.visible_child_name().as_deref() {
            Some("library") => lib.ensure_loaded(),
            Some("search") => search2.focus(),
            _ => {}
        });
    }
    {
        let (lib, stack2) = (library.clone(), stack.clone());
        search.set_open_collection(move |c| {
            stack2.set_visible_child_name("library");
            lib.open(c);
        });
    }

    // Events → widgets.
    {
        let (toasts, busy, window2, p_video, video_picture, stack2) = (
            toasts.clone(),
            busy.clone(),
            window.downgrade(),
            p_video.clone(),
            video_picture.clone(),
            stack.clone(),
        );
        ctl.subscribe(move |ev| match ev {
            AppEvent::Toast(t) => {
                let toast = adw::Toast::builder()
                    .title(glib::markup_escape_text(&t.title))
                    .timeout(if t.priority_high { 6 } else { 3 })
                    .build();
                if t.priority_high {
                    toast.set_priority(adw::ToastPriority::High);
                }
                if let Some(undo) = t.undo.clone() {
                    toast.set_button_label(Some("Undo"));
                    toast.connect_button_clicked(move |_| undo());
                }
                toasts.add_toast(toast);
            }
            AppEvent::Busy(b) => busy.set_visible(*b),
            AppEvent::Raise => {
                if let Some(w) = window2.upgrade() {
                    w.present();
                }
            }
            AppEvent::Video(p) => {
                video_picture.set_paintable(p.as_ref());
                p_video.set_visible(p.is_some());
                if p.is_none() && stack2.visible_child_name().as_deref() == Some("video") {
                    stack2.set_visible_child_name("search");
                }
            }
            _ => {}
        });
    }

    // Window actions.
    let add = |name: &str, f: Box<dyn Fn()>| {
        let a = gio::SimpleAction::new(name, None);
        a.connect_activate(move |_, _| f());
        window.add_action(&a);
    };
    {
        let (ctl, w) = (ctl.clone(), window.downgrade());
        add(
            "accounts",
            Box::new(move || {
                if let Some(w) = w.upgrade() {
                    accounts::present(&ctl, &w);
                }
            }),
        );
    }
    {
        let (search, stack) = (search.clone(), stack.clone());
        add(
            "focus-search",
            Box::new(move || {
                stack.set_visible_child_name("search");
                search.focus();
            }),
        );
    }
    {
        let stack = stack.clone();
        add(
            "show-library",
            Box::new(move || stack.set_visible_child_name("library")),
        );
    }
    {
        let t = queue_toggle.clone();
        add(
            "toggle-queue",
            Box::new(move || t.set_active(!t.is_active())),
        );
    }
    {
        let ctl = ctl.clone();
        add("play-pause", Box::new(move || ctl.toggle_play()));
    }
    {
        let ctl = ctl.clone();
        add("next", Box::new(move || ctl.advance(Advance::User)));
    }
    {
        let ctl = ctl.clone();
        add("previous", Box::new(move || ctl.previous()));
    }
    {
        let (ctl, w) = (ctl.clone(), window.downgrade());
        add(
            "share-current",
            Box::new(move || {
                let (Some(w), Some(e)) = (w.upgrade(), ctl.current_entry()) else {
                    ctl.toast(crate::app::ToastSpec::info("Nothing is playing"));
                    return;
                };
                crate::ui::share_to_discord(&ctl, &w, &[e.track]);
            }),
        );
    }
    {
        let ctl = ctl.clone();
        add(
            "open-artist",
            Box::new(move || {
                if let Some(e) = ctl.current_entry() {
                    crate::ui::open_uri(&ctl, &e.track.artist_page_url());
                }
            }),
        );
    }

    let mini = gio::SimpleAction::new_stateful("mini-mode", None, &false.to_variant());
    {
        let saved = Rc::new(Cell::new((1120, 760)));
        let (w, toasts, exit_mini, compact) = (
            window.downgrade(),
            toasts.clone(),
            exit_mini.clone(),
            now.compact_widgets.clone(),
        );
        mini.connect_activate(move |a, _| {
            let Some(w) = w.upgrade() else { return };
            let on = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            a.set_state(&on.to_variant());
            toasts.set_visible(!on);
            exit_mini.set_visible(on);
            for c in &compact {
                if on {
                    c.set_visible(false);
                } else {
                    c.set_visible(true);
                }
            }
            if on {
                saved.set((w.width(), w.height()));
                w.set_default_size(MINI_SIZE.0, MINI_SIZE.1);
            } else {
                let (sw, sh) = saved.get();
                w.set_default_size(sw.max(640), sh.max(480));
            }
        });
    }
    window.add_action(&mini);
    if ctl.prefs.borrow().mini_mode {
        mini.activate(None);
    }

    // Remember window geometry and layout.
    {
        let (ctl, mini, toggle) = (ctl.clone(), mini.clone(), queue_toggle.clone());
        window.connect_close_request(move |w| {
            let is_mini = mini.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            {
                let mut p = ctl.prefs.borrow_mut();
                p.maximized = w.is_maximized();
                if !is_mini && !p.maximized {
                    let (dw, dh) = w.default_size();
                    p.window_width = dw;
                    p.window_height = dh;
                }
                p.mini_mode = is_mini;
                p.show_queue = toggle.is_active();
            }
            ctl.save_prefs();
            ctl.save_session();
            glib::Propagation::Proceed
        });
    }

    {
        let search = search.clone();
        window.connect_map(move |_| search.focus());
    }
    // Keep the search page's Rc alive with the window.
    let keep = (search, library, queue, now);
    window.connect_destroy(move |_| {
        let _ = &keep;
    });
    window
}
