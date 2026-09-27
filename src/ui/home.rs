//! Home: YouTube Music's personalised shelves ("Quick picks", "Listen again", mixes,
//! albums, podcasts) as horizontal rows of artwork cards. Tapping a song queues it;
//! tapping a playlist/album/artist opens it; every card has `+`.

use crate::app::{Controller, HomeState};
use crate::ui::rows::Artwork;
use adw::prelude::*;
use banshee::model::{Collection, CollectionKind, HomeShelf, SearchItem};
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;

const CARD_PX: i32 = 148;

type OpenCollection = Rc<dyn Fn(Collection)>;

pub struct HomeView {
    pub root: gtk::Box,
    shelves: gtk::Box,
    status: gtk::Stack,
    ctl: Rc<Controller>,
    open_collection: RefCell<Option<OpenCollection>>,
}

impl HomeView {
    pub fn new(ctl: &Rc<Controller>) -> Rc<Self> {
        let shelves = gtk::Box::new(gtk::Orientation::Vertical, 28);
        let status = gtk::Stack::new();
        let loading = adw::Spinner::builder()
            .height_request(32)
            .width_request(32)
            .build();
        let loading_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        loading_box.set_margin_top(48);
        loading_box.append(&loading);
        status.add_named(&loading_box, Some("loading"));
        status.add_named(&shelves, Some("shelves"));
        let failed = adw::StatusPage::builder()
            .icon_name("network-error-symbolic")
            .title("Home Is Unavailable")
            .build();
        failed.add_css_class("compact");
        let retry = gtk::Button::builder()
            .label("Try Again")
            .halign(gtk::Align::Center)
            .build();
        retry.add_css_class("pill");
        failed.set_child(Some(&retry));
        status.add_named(&failed, Some("failed"));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let clamp = adw::Clamp::builder()
            .maximum_size(1100)
            .child(&status)
            .build();
        root.append(&clamp);

        let this = Rc::new(Self {
            root,
            shelves,
            status,
            ctl: ctl.clone(),
            open_collection: RefCell::new(None),
        });
        let weak = Rc::downgrade(&this);
        retry.connect_clicked(move |_| {
            if let Some(h) = weak.upgrade() {
                h.load(true);
            }
        });
        let weak = Rc::downgrade(&this);
        ctl.subscribe(move |ev| {
            if let crate::app::AppEvent::AccountsChanged = ev {
                if let Some(h) = weak.upgrade() {
                    h.load(true);
                }
            }
        });
        this.load(false);
        this
    }

    pub fn set_open_collection(&self, f: Rc<dyn Fn(Collection)>) {
        *self.open_collection.borrow_mut() = Some(f);
    }

    fn load(self: &Rc<Self>, force: bool) {
        let weak = Rc::downgrade(self);
        let failed_desc = self
            .status
            .child_by_name("failed")
            .and_downcast::<adw::StatusPage>();
        self.ctl.load_home(force, move |state| {
            let Some(h) = weak.upgrade() else { return };
            match state {
                HomeState::Loading => h.status.set_visible_child_name("loading"),
                HomeState::Ready(shelves) => {
                    h.render(&shelves);
                    h.status.set_visible_child_name("shelves");
                }
                HomeState::Failed(msg) => {
                    if let Some(p) = &failed_desc {
                        p.set_description(Some(&glib::markup_escape_text(&msg)));
                    }
                    h.status.set_visible_child_name("failed");
                }
            }
        });
    }

    fn render(self: &Rc<Self>, shelves: &[HomeShelf]) {
        while let Some(c) = self.shelves.first_child() {
            self.shelves.remove(&c);
        }
        for shelf in shelves {
            self.shelves.append(&self.shelf(shelf));
        }
    }

    fn shelf(self: &Rc<Self>, shelf: &HomeShelf) -> gtk::Box {
        let title = gtk::Label::builder()
            .label(&shelf.title)
            .xalign(0.0)
            .build();
        title.add_css_class("title-3");
        let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
        heading.set_hexpand(true);
        if let Some(s) = shelf.strapline.as_ref().filter(|s| !s.is_empty()) {
            let l = gtk::Label::builder().label(s).xalign(0.0).build();
            l.add_css_class("caption");
            l.add_css_class("dim-label");
            heading.append(&l);
        }
        heading.append(&title);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        for item in &shelf.items {
            row.append(&self.card(item));
        }
        let scroller = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::External)
            .child(&row)
            .build();
        scroller.add_css_class("undershoot-start");

        let page = |dir: f64| {
            let s = scroller.clone();
            move |_: &gtk::Button| {
                let adj = s.hadjustment();
                let target = (adj.value() + dir * adj.page_size() * 0.85)
                    .clamp(adj.lower(), adj.upper() - adj.page_size());
                adj.set_value(target);
            }
        };
        let back = crate::ui::player_widgets::icon_button("go-previous-symbolic", "Scroll Back");
        back.connect_clicked(page(-1.0));
        let fwd = crate::ui::player_widgets::icon_button("go-next-symbolic", "Scroll Forward");
        fwd.connect_clicked(page(1.0));
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header.set_margin_start(12);
        header.set_margin_end(12);
        header.append(&heading);
        header.append(&back);
        header.append(&fwd);

        let wrap = gtk::Box::new(gtk::Orientation::Vertical, 10);
        row.set_margin_start(12);
        row.set_margin_end(12);
        wrap.append(&header);
        wrap.append(&scroller);
        wrap
    }

    fn card(self: &Rc<Self>, item: &SearchItem) -> gtk::Widget {
        let (title, subtitle, thumb, icon, round) = match item {
            SearchItem::Track(t) => (
                t.title.clone(),
                t.artist.clone(),
                t.thumbnail_url.clone(),
                t.kind.icon_name(),
                false,
            ),
            SearchItem::Collection(c) => (
                c.title.clone(),
                c.subtitle.clone(),
                c.thumbnail_url.clone(),
                c.kind.icon_name(),
                c.kind == CollectionKind::Artist,
            ),
        };
        let art = Artwork::new(CARD_PX);
        art.set_icon(icon);
        art.set_round(round);
        art.load(&self.ctl, thumb.as_deref());

        let add = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text(match item {
                SearchItem::Track(_) => "Add to Queue",
                SearchItem::Collection(_) => "Add All to Queue",
            })
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .margin_end(6)
            .margin_bottom(6)
            .build();
        add.add_css_class("circular");
        add.add_css_class("osd");
        add.add_css_class("card-add");
        add.update_property(&[gtk::accessible::Property::Label("Add to queue")]);
        {
            let (ctl, item) = (self.ctl.clone(), item.clone());
            add.connect_clicked(move |_| match &item {
                SearchItem::Track(t) => ctl.enqueue(t.clone()),
                SearchItem::Collection(c) => ctl.enqueue_collection(c.clone()),
            });
        }

        let t = gtk::Label::builder()
            .label(&title)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(16)
            .width_request(CARD_PX)
            .build();
        t.add_css_class("card-title");
        let sub = gtk::Label::builder()
            .label(&subtitle)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(18)
            .width_request(CARD_PX)
            .build();
        sub.add_css_class("caption");
        sub.add_css_class("dim-label");

        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        body.append(&art.root);
        body.append(&t);
        body.append(&sub);
        let button = gtk::Button::builder()
            .child(&body)
            .tooltip_text(&title)
            .build();
        button.add_css_class("flat");
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "{title}, {subtitle}"
        ))]);
        {
            let (ctl, item, weak) = (self.ctl.clone(), item.clone(), Rc::downgrade(self));
            button.connect_clicked(move |_| match &item {
                // Queueing is the default action.
                SearchItem::Track(t) => ctl.enqueue(t.clone()),
                SearchItem::Collection(c) => {
                    if let Some(f) = weak
                        .upgrade()
                        .and_then(|h| h.open_collection.borrow().clone())
                    {
                        f(c.clone());
                    }
                }
            });
        }
        // `+` is a sibling over the card (not a button inside a button), so it is its own
        // focus stop and accessible element. It sits over the artwork's corner.
        add.set_valign(gtk::Align::Start);
        add.set_margin_top(CARD_PX - 36 + 6);
        add.set_margin_end(12);
        let card = gtk::Overlay::builder().child(&button).build();
        card.add_overlay(&add);
        card.add_css_class("home-card");
        card.upcast()
    }
}
