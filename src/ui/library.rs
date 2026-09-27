//! Library: signed-in users' playlists, artists, albums, liked songs and podcasts per source,
//! from the stale-while-revalidate cache (ADR 0011).

use crate::app::{Controller, LibraryState};
use crate::ui::rows::{Artwork, ItemRow, RowItem, RowMode};
use adw::prelude::*;
use banshee::model::{Collection, CollectionKind, SearchItem, SourceKind, Track};
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

pub struct LibraryView {
    pub nav: adw::NavigationView,
    ctl: Rc<Controller>,
    refresh_btn: gtk::Button,
    loaded: std::cell::Cell<bool>,
    source_boxes: RefCell<Vec<(SourceKind, gtk::Box)>>,
}

impl LibraryView {
    pub fn new(ctl: &Rc<Controller>) -> Rc<Self> {
        let groups = gtk::Box::new(gtk::Orientation::Vertical, 24);
        groups.set_margin_top(24);
        groups.set_margin_bottom(24);
        groups.set_margin_start(12);
        groups.set_margin_end(12);

        let refresh_btn = gtk::Button::builder()
            .label("Refresh")
            .halign(gtk::Align::End)
            .build();
        refresh_btn.add_css_class("pill");
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let heading = gtk::Label::builder()
            .label("Your Library")
            .xalign(0.0)
            .hexpand(true)
            .build();
        heading.add_css_class("title-1");
        top.append(&heading);
        top.append(&refresh_btn);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
        content.append(&top);
        content.append(&groups);
        content.set_margin_top(24);
        content.set_margin_start(12);
        content.set_margin_end(12);
        let clamp = adw::Clamp::builder()
            .maximum_size(820)
            .child(&content)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&clamp)
            .build();
        let root_page = adw::NavigationPage::builder()
            .title("Library")
            .tag("library")
            .child(&scroller)
            .build();
        let nav = adw::NavigationView::new();
        nav.add(&root_page);

        let mut boxes = Vec::new();
        for s in [SourceKind::YouTubeMusic, SourceKind::Spotify] {
            let b = gtk::Box::new(gtk::Orientation::Vertical, 24);
            groups.append(&b);
            boxes.push((s, b));
        }

        let this = Rc::new(Self {
            nav,
            ctl: ctl.clone(),
            refresh_btn,
            loaded: std::cell::Cell::new(false),
            source_boxes: RefCell::new(boxes),
        });
        let weak = Rc::downgrade(&this);
        this.refresh_btn.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.load(true);
            }
        });
        let weak = Rc::downgrade(&this);
        ctl.subscribe(move |ev| {
            if let crate::app::AppEvent::AccountsChanged = ev {
                if let Some(v) = weak.upgrade() {
                    v.load(false);
                }
            }
        });
        this
    }

    /// Load (or show cached) library data; called when the Library view is first shown.
    pub fn ensure_loaded(self: &Rc<Self>) {
        if !self.loaded.replace(true) {
            self.load(false);
        }
    }

    pub fn load(self: &Rc<Self>, force: bool) {
        for (source, container) in self.source_boxes.borrow().iter() {
            let weak = Rc::downgrade(self);
            let (source, container) = (*source, container.clone());
            self.ctl.load_library(source, force, move |state| {
                if let Some(v) = weak.upgrade() {
                    v.render_source(source, &container, state);
                }
            });
        }
    }

    fn render_source(
        self: &Rc<Self>,
        source: SourceKind,
        container: &gtk::Box,
        state: LibraryState,
    ) {
        while let Some(c) = container.first_child() {
            container.remove(&c);
        }
        let name = match source {
            SourceKind::YouTubeMusic => "YouTube Music",
            SourceKind::Spotify => "Spotify",
        };
        match state {
            LibraryState::SignedOut => {
                let g = adw::PreferencesGroup::builder().title(name).build();
                let row = adw::ActionRow::builder()
                    .title("Not signed in")
                    .subtitle(format!(
                        "Sign in to see your {name} playlists, artists, albums and podcasts."
                    ))
                    .build();
                let btn = gtk::Button::builder()
                    .label("Sign In…")
                    .valign(gtk::Align::Center)
                    .action_name("win.accounts")
                    .build();
                row.add_suffix(&btn);
                g.add(&row);
                container.append(&g);
            }
            LibraryState::Loading => {
                let g = adw::PreferencesGroup::builder().title(name).build();
                let row = adw::ActionRow::builder()
                    .title("Loading your library…")
                    .build();
                row.add_suffix(&adw::Spinner::new());
                g.add(&row);
                container.append(&g);
            }
            LibraryState::Failed(msg) => {
                let g = adw::PreferencesGroup::builder().title(name).build();
                let row = adw::ActionRow::builder()
                    .title("Couldn’t load the library")
                    .subtitle(glib::markup_escape_text(&msg))
                    .build();
                row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
                let btn = gtk::Button::builder()
                    .label("Retry")
                    .valign(gtk::Align::Center)
                    .build();
                let weak = Rc::downgrade(self);
                btn.connect_clicked(move |_| {
                    if let Some(v) = weak.upgrade() {
                        v.load(true);
                    }
                });
                row.add_suffix(&btn);
                g.add(&row);
                container.append(&g);
            }
            LibraryState::Ready {
                sections,
                refreshing,
            } => {
                for (i, section) in sections.iter().enumerate() {
                    if section.collections.is_empty() {
                        continue;
                    }
                    let g = adw::PreferencesGroup::builder()
                        .title(format!("{name} · {}", section.title))
                        .description(format!("{}", section.collections.len()))
                        .build();
                    if i == 0 && refreshing {
                        g.set_header_suffix(Some(
                            &adw::Spinner::builder().tooltip_text("Refreshing").build(),
                        ));
                    }
                    for c in &section.collections {
                        g.add(&self.collection_row(c));
                    }
                    container.append(&g);
                }
            }
        }
    }

    fn collection_row(self: &Rc<Self>, c: &Collection) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&c.title))
            .subtitle(glib::markup_escape_text(&c.subtitle))
            .activatable(true)
            .build();
        let art = Artwork::new(40);
        art.set_icon(c.kind.icon_name());
        art.set_round(c.kind == CollectionKind::Artist);
        art.load(&self.ctl, c.thumbnail_url.as_deref());
        row.add_prefix(&art.root);
        let add = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add All to Queue")
            .valign(gtk::Align::Center)
            .build();
        add.add_css_class("flat");
        add.update_property(&[gtk::accessible::Property::Label("Add all to queue")]);
        {
            let (ctl, c) = (self.ctl.clone(), c.clone());
            add.connect_clicked(move |_| ctl.enqueue_collection(c.clone()));
        }
        row.add_suffix(&add);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let weak = Rc::downgrade(self);
        let c = c.clone();
        row.connect_activated(move |_| {
            if let Some(v) = weak.upgrade() {
                v.open(c.clone());
            }
        });
        row
    }

    /// Push a Collection page (also used by search results).
    pub fn open(self: &Rc<Self>, c: Collection) {
        let page = CollectionPage::new(&self.ctl, c.clone());
        // Page state lives in the widgets' signal closures.
        let nav_page = adw::NavigationPage::builder()
            .title(&c.title)
            .child(&page.root)
            .build();
        self.nav.push(&nav_page);
    }
}

struct CollectionPage {
    root: gtk::ScrolledWindow,
}

impl CollectionPage {
    fn new(ctl: &Rc<Controller>, c: Collection) -> Self {
        let art = Artwork::new(160);
        art.set_icon(c.kind.icon_name());
        art.set_round(c.kind == CollectionKind::Artist);
        art.load(ctl, c.thumbnail_url.as_deref());

        let title = gtk::Label::builder()
            .label(&c.title)
            .wrap(true)
            .xalign(0.0)
            .build();
        title.add_css_class("title-1");
        let subtitle = gtk::Label::builder()
            .label(&c.subtitle)
            .wrap(true)
            .xalign(0.0)
            .build();
        subtitle.add_css_class("dim-label");
        let count = gtk::Label::builder().xalign(0.0).build();
        count.add_css_class("caption");
        count.add_css_class("dim-label");

        let add_all = gtk::Button::builder()
            .label("Add All to Queue")
            .sensitive(false)
            .build();
        add_all.add_css_class("pill");
        add_all.add_css_class("suggested-action");
        let play_all = gtk::Button::builder()
            .label("Play Next")
            .sensitive(false)
            .build();
        play_all.add_css_class("pill");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        buttons.append(&add_all);
        buttons.append(&play_all);

        let info = gtk::Box::new(gtk::Orientation::Vertical, 6);
        info.set_valign(gtk::Align::Center);
        info.append(&title);
        info.append(&subtitle);
        info.append(&count);
        info.append(&buttons);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        header.append(&art.root);
        header.append(&info);

        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::NoSelection::new(Some(store.clone()));
        let factory = gtk::SignalListItemFactory::new();
        {
            let ctl_outer = ctl.clone();
            let ctl = ctl_outer.clone();
            factory.connect_setup(move |_, item| {
                if let Some(li) = item.downcast_ref::<gtk::ListItem>() {
                    li.set_child(Some(&ItemRow::new(&ctl, RowMode::Result)));
                }
            });
            let ctl = ctl_outer.clone();
            factory.connect_bind(move |_, item| {
                let Some(li) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let (Some(row), Some(obj)) = (
                    li.child().and_downcast::<ItemRow>(),
                    li.item().and_downcast::<glib::BoxedAnyObject>(),
                ) else {
                    return;
                };
                let t = obj.borrow::<Track>().clone();
                row.bind(&ctl, RowItem::Result(SearchItem::Track(t)), false);
            });
        }
        let list = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .build();
        list.add_css_class("rich-list");
        list.add_css_class("results");
        {
            let (ctl, store) = (ctl.clone(), store.clone());
            list.connect_activate(move |_, pos| {
                if let Some(obj) = store.item(pos).and_downcast::<glib::BoxedAnyObject>() {
                    ctl.enqueue(obj.borrow::<Track>().clone());
                }
            });
        }

        let stack = gtk::Stack::new();
        let loading = adw::StatusPage::builder().title("Loading…").build();
        loading.set_paintable(Some(&adw::SpinnerPaintable::new(Some(&loading))));
        stack.add_named(&loading, Some("loading"));
        stack.add_named(&list, Some("list"));
        let error = adw::StatusPage::builder()
            .icon_name("network-error-symbolic")
            .title("Couldn’t Load")
            .build();
        let retry = gtk::Button::builder()
            .label("Retry")
            .halign(gtk::Align::Center)
            .build();
        retry.add_css_class("pill");
        error.set_child(Some(&retry));
        stack.add_named(&error, Some("error"));
        stack.add_named(
            &adw::StatusPage::builder()
                .icon_name("view-list-symbolic")
                .title("Nothing Here")
                .build(),
            Some("empty"),
        );

        let body = gtk::Box::new(gtk::Orientation::Vertical, 24);
        body.set_margin_top(24);
        body.set_margin_bottom(24);
        body.set_margin_start(12);
        body.set_margin_end(12);
        body.append(&header);
        body.append(&stack);
        let clamp = adw::Clamp::builder().maximum_size(820).child(&body).build();
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&clamp)
            .build();

        let tracks: Rc<RefCell<Vec<Track>>> = Rc::default();
        let load: Rc<dyn Fn(bool)> = {
            let (ctl, c, stack, store, tracks, count, error, add_all, play_all) = (
                ctl.clone(),
                c.clone(),
                stack.clone(),
                store.clone(),
                tracks.clone(),
                count.clone(),
                error.clone(),
                add_all.clone(),
                play_all.clone(),
            );
            Rc::new(move |force| {
                if store.n_items() == 0 {
                    stack.set_visible_child_name("loading");
                }
                let (stack, store, tracks, count, error, add_all, play_all) = (
                    stack.clone(),
                    store.clone(),
                    tracks.clone(),
                    count.clone(),
                    error.clone(),
                    add_all.clone(),
                    play_all.clone(),
                );
                let (ctl2, title) = (ctl.clone(), c.title.clone());
                ctl.load_collection(c.clone(), force, move |r, _refreshing| match r {
                    Ok(list) => {
                        let objs: Vec<glib::BoxedAnyObject> = list
                            .iter()
                            .cloned()
                            .map(glib::BoxedAnyObject::new)
                            .collect();
                        store.splice(0, store.n_items(), &objs);
                        let total: u64 = list
                            .iter()
                            .filter_map(|t| t.duration_secs)
                            .map(u64::from)
                            .sum();
                        count.set_label(&if total > 0 {
                            format!(
                                "{} items · {}",
                                list.len(),
                                banshee::model::format_total(total)
                            )
                        } else {
                            format!("{} items", list.len())
                        });
                        add_all.set_sensitive(!list.is_empty());
                        play_all.set_sensitive(!list.is_empty());
                        stack.set_visible_child_name(if list.is_empty() {
                            "empty"
                        } else {
                            "list"
                        });
                        *tracks.borrow_mut() = list;
                    }
                    Err(e) => {
                        if store.n_items() == 0 {
                            error.set_description(Some(&glib::markup_escape_text(&e)));
                            stack.set_visible_child_name("error");
                        } else {
                            ctl2.toast_error(format!("Couldn’t refresh “{title}” — {e}"));
                        }
                    }
                });
            })
        };
        load(false);
        {
            let load = load.clone();
            retry.connect_clicked(move |_| load(true));
        }
        {
            let (ctl, tracks, label) = (ctl.clone(), tracks.clone(), c.title.clone());
            add_all.connect_clicked(move |_| ctl.enqueue_many(tracks.borrow().clone(), &label));
        }
        {
            let (ctl, tracks, label) = (ctl.clone(), tracks.clone(), c.title.clone());
            play_all.connect_clicked(move |_| ctl.play_next_many(tracks.borrow().clone(), &label));
        }
        Self { root }
    }
}
