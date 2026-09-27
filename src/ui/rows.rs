//! Row widgets shared by search results, collections and the queue.

use crate::app::Controller;
use adw::prelude::*;
use adw::subclass::prelude::*;
use banshee::artwork::sized_thumbnail;
use banshee::model::{CollectionKind, SearchItem, SourceKind, Track};
use banshee::queue::QueueEntry;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Artwork thumbnail with a symbolic placeholder; loads through the Artwork Store.
#[derive(Clone)]
pub struct Artwork {
    pub root: gtk::Overlay,
    picture: gtk::Picture,
    placeholder: gtk::Image,
    url: Rc<RefCell<Option<String>>>,
    px: i32,
}

impl Artwork {
    pub fn new(px: i32) -> Self {
        let placeholder = gtk::Image::builder()
            .icon_name("folder-music-symbolic")
            .pixel_size(px / 2)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        placeholder.add_css_class("dim-label");
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .width_request(px)
            .height_request(px)
            .build();
        let root = gtk::Overlay::builder()
            .child(&placeholder)
            .width_request(px)
            .height_request(px)
            .overflow(gtk::Overflow::Hidden)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .build();
        root.add_overlay(&picture);
        root.add_css_class("artwork");
        Self {
            root,
            picture,
            placeholder,
            url: Rc::default(),
            px,
        }
    }

    pub fn set_icon(&self, icon: &str) {
        self.placeholder.set_icon_name(Some(icon));
    }

    pub fn set_round(&self, round: bool) {
        if round {
            self.root.add_css_class("round");
        } else {
            self.root.remove_css_class("round");
        }
    }

    /// Show a live paintable (video) instead of a thumbnail.
    pub fn set_paintable(&self, p: Option<&gdk::Paintable>) {
        *self.url.borrow_mut() = None;
        self.picture.set_paintable(p);
    }

    pub fn load(&self, ctl: &Rc<Controller>, url: Option<&str>) {
        let url = url.map(|u| sized_thumbnail(u, (self.px * 2).max(120) as u32));
        if *self.url.borrow() == url && self.picture.paintable().is_some() {
            return;
        }
        *self.url.borrow_mut() = url.clone();
        let Some(url) = url else {
            self.picture.set_paintable(gdk::Paintable::NONE);
            return;
        };
        if let Some(t) = ctl.artwork.cached(&url) {
            self.picture.set_paintable(Some(&t));
            return;
        }
        self.picture.set_paintable(gdk::Paintable::NONE);
        let store = ctl.artwork.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let res = store.load(&url).await;
            // The row may have been rebound to another item while loading.
            if this.url.borrow().as_deref() != Some(url.as_str()) {
                return;
            }
            match res {
                Ok(t) => this.picture.set_paintable(Some(&t)),
                Err(e) => log::debug!("artwork {url}: {e}"),
            }
        });
    }
}

type RowAction = Rc<dyn Fn(&Rc<Controller>, &RowItem)>;

/// What a row represents and which actions it offers.
#[derive(Clone, Debug)]
pub enum RowItem {
    Result(SearchItem),
    Queue { entry: QueueEntry, index: usize },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowMode {
    Result,
    /// A search result in Mini Mode's quick-add: just artwork, text and `+`.
    QuickAdd,
    Queue,
}

/// Second line of a track: the artist, plus the length when known. The kind of item is
/// shown by its artwork placeholder and badge, never as a word.
pub fn subtitle_for(t: &Track) -> String {
    let d = t.duration_label();
    match (t.artist.is_empty(), d.is_empty()) {
        (false, false) => format!("{} · {d}", t.artist),
        (false, true) => t.artist.clone(),
        (true, _) => d,
    }
}

/// The album (or the podcast, for an episode) as its own line, when known.
pub fn album_line(t: &Track) -> Option<String> {
    t.album
        .clone()
        .filter(|a| !a.trim().is_empty() && *a != t.title)
}

fn source_badge(s: SourceKind) -> &'static str {
    s.label()
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ItemRow {
        pub art: RefCell<Option<Artwork>>,
        pub title: RefCell<Option<gtk::Label>>,
        pub subtitle: RefCell<Option<gtk::Label>>,
        pub album: RefCell<Option<gtk::Label>>,
        pub badge: RefCell<Option<gtk::Label>>,
        pub primary: RefCell<Option<gtk::Button>>,
        pub playing: RefCell<Option<gtk::Image>>,
        pub item: RefCell<Option<RowItem>>,
        pub mode: Cell<Option<RowMode>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ItemRow {
        const NAME: &'static str = "BansheeItemRow";
        type Type = super::ItemRow;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for ItemRow {}
    impl WidgetImpl for ItemRow {}
    impl BoxImpl for ItemRow {}
}

glib::wrapper! {
    pub struct ItemRow(ObjectSubclass<imp::ItemRow>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl ItemRow {
    pub fn new(ctl: &Rc<Controller>, mode: RowMode) -> Self {
        let row: Self = glib::Object::builder()
            .property("orientation", gtk::Orientation::Horizontal)
            .property("spacing", 12)
            .build();
        row.add_css_class("item-row");
        let imp = row.imp();
        imp.mode.set(Some(mode));

        let art = Artwork::new(48);
        row.append(&art.root);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        title.add_css_class("item-title");
        let subtitle = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        subtitle.add_css_class("dim-label");
        subtitle.add_css_class("caption");
        let album = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .visible(false)
            .build();
        album.add_css_class("dim-label");
        album.add_css_class("caption");
        album.add_css_class("item-album");
        text.append(&title);
        text.append(&subtitle);
        text.append(&album);
        row.append(&text);

        let playing = gtk::Image::from_icon_name("media-playback-start-symbolic");
        playing.set_visible(false);
        playing.add_css_class("accent");
        playing.set_tooltip_text(Some("Now playing"));
        row.append(&playing);

        let badge = gtk::Label::new(None);
        badge.add_css_class("source-badge");
        badge.set_valign(gtk::Align::Center);
        row.append(&badge);

        let (icon, tip) = match mode {
            RowMode::Result | RowMode::QuickAdd => ("list-add-symbolic", "Add to Queue"),
            RowMode::Queue => ("list-remove-symbolic", "Remove from Queue"),
        };
        let primary = gtk::Button::builder()
            .icon_name(icon)
            .tooltip_text(tip)
            .valign(gtk::Align::Center)
            .build();
        primary.add_css_class("flat");
        primary.add_css_class("circular");
        primary.update_property(&[gtk::accessible::Property::Label(tip)]);
        row.append(&primary);

        let menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("More")
            .valign(gtk::Align::Center)
            .build();
        menu.add_css_class("flat");
        menu.add_css_class("circular");
        // Mini's quick-add keeps text room: its menu is right-click / long-press only.
        menu.set_visible(mode != RowMode::QuickAdd);
        row.append(&menu);

        // Row-local actions used by the menu; they read whatever item is bound now.
        let group = gio::SimpleActionGroup::new();
        let add_action = |name: &str, f: RowAction| {
            let a = gio::SimpleAction::new(name, None);
            let (weak_row, ctl) = (row.downgrade(), ctl.clone());
            a.connect_activate(move |_, _| {
                if let Some(r) = weak_row.upgrade() {
                    if let Some(item) = r.imp().item.borrow().clone() {
                        f(&ctl, &item);
                    }
                }
            });
            group.add_action(&a);
        };
        add_action(
            "play-now",
            Rc::new(|c, i| match i {
                RowItem::Result(SearchItem::Track(t)) => c.play_now(t.clone()),
                RowItem::Queue { entry, .. } => c.play_entry_id(entry.id),
                RowItem::Result(SearchItem::Collection(_)) => {}
            }),
        );
        add_action(
            "play-next",
            Rc::new(|c, i| match i {
                RowItem::Result(SearchItem::Track(t)) => c.play_next(t.clone()),
                RowItem::Queue { entry, .. } => c.play_next(entry.track.clone()),
                RowItem::Result(SearchItem::Collection(_)) => {}
            }),
        );
        add_action(
            "move-up",
            Rc::new(|c, i| {
                if let RowItem::Queue { index, .. } = i {
                    if *index > 0 {
                        c.move_entry(*index, index - 1);
                    }
                }
            }),
        );
        add_action(
            "move-down",
            Rc::new(|c, i| {
                if let RowItem::Queue { index, .. } = i {
                    if index + 1 < c.queue_len() {
                        c.move_entry(*index, index + 1);
                    }
                }
            }),
        );
        add_action(
            "open-artist",
            Rc::new(|c, i| {
                if let Some(t) = item_track(i) {
                    crate::ui::open_uri(c, &t.artist_page_url());
                }
            }),
        );
        add_action(
            "copy-link",
            Rc::new(|c, i| {
                let link = match i {
                    RowItem::Result(SearchItem::Collection(col)) => crate::ui::collection_url(col),
                    other => item_track(other).map(|t| t.web_url()).unwrap_or_default(),
                };
                if let Some(d) = gdk::Display::default() {
                    d.clipboard().set_text(&link);
                    c.toast(crate::app::ToastSpec::info("Link copied"));
                }
            }),
        );
        row.insert_action_group("row", Some(&group));

        let model = gio::Menu::new();
        match mode {
            RowMode::Result | RowMode::QuickAdd => {
                model.append(Some("Play Now"), Some("row.play-now"));
                model.append(Some("Play Next"), Some("row.play-next"));
            }
            RowMode::Queue => {
                model.append(Some("Play"), Some("row.play-now"));
                model.append(Some("Move Up"), Some("row.move-up"));
                model.append(Some("Move Down"), Some("row.move-down"));
            }
        }
        let links = gio::Menu::new();
        links.append(
            Some("Open Artist on YouTube Music"),
            Some("row.open-artist"),
        );
        links.append(Some("Copy Link"), Some("row.copy-link"));
        model.append_section(None, &links);
        menu.set_menu_model(Some(&model));
        // Context menu (secondary click or long press) on every row.
        let popover = gtk::PopoverMenu::from_model(Some(&model));
        popover.set_parent(&row);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        let open_at = {
            let popover = popover.clone();
            move |x: f64, y: f64| {
                popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                popover.popup();
            }
        };
        let click = gtk::GestureClick::builder()
            .button(gdk::BUTTON_SECONDARY)
            .build();
        {
            let open_at = open_at.clone();
            click.connect_pressed(move |g, _, x, y| {
                g.set_state(gtk::EventSequenceState::Claimed);
                open_at(x, y);
            });
        }
        row.add_controller(click);
        let press = gtk::GestureLongPress::new();
        {
            let open_at = open_at.clone();
            press.connect_pressed(move |_, x, y| open_at(x, y));
        }
        row.add_controller(press);
        {
            // Unparent the popover with the row so it doesn't leak.
            let popover = popover.clone();
            row.connect_destroy(move |_| popover.unparent());
        }

        {
            let (weak_row, ctl) = (row.downgrade(), ctl.clone());
            primary.connect_clicked(move |_| {
                let Some(r) = weak_row.upgrade() else { return };
                let item = r.imp().item.borrow().clone();
                match item {
                    Some(RowItem::Result(SearchItem::Track(t))) => ctl.enqueue(t),
                    Some(RowItem::Result(SearchItem::Collection(c))) => ctl.enqueue_collection(c),
                    Some(RowItem::Queue { entry, .. }) => ctl.remove_entry(entry.id, true),
                    None => {}
                }
            });
        }

        *imp.art.borrow_mut() = Some(art);
        *imp.title.borrow_mut() = Some(title);
        *imp.subtitle.borrow_mut() = Some(subtitle);
        *imp.album.borrow_mut() = Some(album);
        *imp.badge.borrow_mut() = Some(badge);
        *imp.primary.borrow_mut() = Some(primary);
        *imp.playing.borrow_mut() = Some(playing);
        row
    }

    /// The row's `+` / remove button (so pages can add behaviour, e.g. remembering a query).
    pub fn primary_button(&self) -> Option<gtk::Button> {
        self.imp().primary.borrow().clone()
    }

    pub fn item(&self) -> Option<RowItem> {
        self.imp().item.borrow().clone()
    }

    pub fn bind(&self, ctl: &Rc<Controller>, item: RowItem, is_current: bool) {
        let imp = self.imp();
        let album = match &item {
            RowItem::Result(SearchItem::Track(t))
            | RowItem::Queue {
                entry: QueueEntry { track: t, .. },
                ..
            } => album_line(t),
            RowItem::Result(SearchItem::Collection(_)) => None,
        };
        if let Some(l) = imp.album.borrow().as_ref() {
            l.set_label(album.as_deref().unwrap_or_default());
            l.set_visible(album.is_some());
        }
        let (title, subtitle, thumb, icon, source, round) = match &item {
            RowItem::Result(SearchItem::Track(t))
            | RowItem::Queue {
                entry: QueueEntry { track: t, .. },
                ..
            } => (
                t.title.clone(),
                subtitle_for(t),
                t.thumbnail_url.clone(),
                t.kind.icon_name(),
                t.source,
                false,
            ),
            RowItem::Result(SearchItem::Collection(c)) => (
                c.title.clone(),
                c.subtitle.clone(),
                c.thumbnail_url.clone(),
                c.kind.icon_name(),
                c.source,
                c.kind == CollectionKind::Artist,
            ),
        };
        if let Some(l) = imp.title.borrow().as_ref() {
            l.set_label(&title);
            l.set_tooltip_text(Some(&title));
        }
        if let Some(l) = imp.subtitle.borrow().as_ref() {
            if imp.mode.get() == Some(RowMode::Queue) {
                l.set_label(&format!("{subtitle} · {}", source_badge(source)));
            } else {
                l.set_label(&subtitle);
            }
        }
        if let Some(b) = imp.badge.borrow().as_ref() {
            b.set_label(source_badge(source));
            b.set_css_classes(&["source-badge", source.slug()]);
            // The queue sidebar is narrow: the source goes into the subtitle instead.
            b.set_visible(imp.mode.get() == Some(RowMode::Result));
        }
        if let Some(p) = imp.playing.borrow().as_ref() {
            p.set_visible(is_current);
        }
        if let Some(b) = imp.primary.borrow().as_ref() {
            let tip = match (&item, imp.mode.get()) {
                (RowItem::Result(SearchItem::Collection(_)), _) => "Add All to Queue",
                (_, Some(RowMode::Queue)) => "Remove from Queue",
                _ => "Add to Queue",
            };
            b.set_tooltip_text(Some(tip));
            b.update_property(&[gtk::accessible::Property::Label(tip)]);
        }
        if let Some(a) = imp.art.borrow().as_ref() {
            a.set_icon(icon);
            a.set_round(round);
            a.load(ctl, thumb.as_deref());
        }
        self.update_property(&[gtk::accessible::Property::Label(&format!(
            "{title}, {subtitle}"
        ))]);
        if is_current {
            self.add_css_class("now-playing");
        } else {
            self.remove_css_class("now-playing");
        }
        *imp.item.borrow_mut() = Some(item);
    }
}

pub fn item_track(i: &RowItem) -> Option<&Track> {
    match i {
        RowItem::Result(SearchItem::Track(t)) => Some(t),
        RowItem::Queue { entry, .. } => Some(&entry.track),
        RowItem::Result(SearchItem::Collection(_)) => None,
    }
}
