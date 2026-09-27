//! Search page: the headline feature. Focused entry, per-keystroke local fuzzy results,
//! debounced remote searches merged into one ranked list, Enter / `+` to queue.

use crate::app::Controller;
use crate::ui::rows::{ItemRow, RowItem, RowMode};
use adw::prelude::*;
use banshee::model::{Collection, SearchFilter, SearchItem, SourceKind};
use banshee::runtime::run;
use banshee::sources::SourceError;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(120);
const SETTLE: Duration = Duration::from_millis(1800);

pub struct SearchPage {
    pub root: gtk::Box,
    pub entry: gtk::SearchEntry,
    ctl: Rc<Controller>,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    stack: gtk::Stack,
    recent: adw::WrapBox,
    compact: bool,
    home: Option<Rc<crate::ui::home::HomeView>>,
    recent_box: gtk::Box,
    filters: adw::ToggleGroup,
    spinner: adw::Spinner,
    banner: adw::Banner,
    filter: Cell<SearchFilter>,
    generation: Cell<u64>,
    pending: Cell<u32>,
    remote: RefCell<HashMap<SourceKind, Vec<SearchItem>>>,
    errors: RefCell<HashMap<SourceKind, String>>,
    debounce: RefCell<Option<glib::SourceId>>,
    /// Query the list was last rendered for; the highlight is kept across re-renders of it.
    rendered_query: RefCell<String>,
    /// The user moved the highlight with Up/Down for the current query.
    user_moved: Cell<bool>,
    /// Enter was pressed before any result existed for this generation: queue the top
    /// result as soon as one arrives (`true` = play next).
    pending_enter: Cell<Option<(u64, bool)>>,
    /// Records the query in Recent Searches once the user has settled on its results.
    settle: RefCell<Option<glib::SourceId>>,
    open_collection: RefCell<Option<OpenCollection>>,
}

type OpenCollection = Box<dyn Fn(Collection)>;

/// The next few Queue Entries, tap to jump to one (Mini Mode quick-add).
fn up_next_box(ctl: &Rc<Controller>) -> gtk::Box {
    let title = gtk::Label::builder().label("Up Next").xalign(0.0).build();
    title.add_css_class("heading");
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("up-next-list");
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.append(&title);
    root.append(&list);
    let render = {
        let (ctl, list, root) = (Rc::downgrade(ctl), list.clone(), root.clone());
        move || {
            let Some(ctl) = ctl.upgrade() else { return };
            list.remove_all();
            let start = ctl.current_index().map_or(0, |i| i + 1);
            let upcoming: Vec<_> = ctl
                .queue_entries()
                .into_iter()
                .skip(start)
                .take(5)
                .collect();
            root.set_visible(!upcoming.is_empty());
            for (offset, e) in upcoming.into_iter().enumerate() {
                let row = adw::ActionRow::builder()
                    .title(glib::markup_escape_text(&e.track.title))
                    .subtitle(glib::markup_escape_text(&e.track.artist))
                    .activatable(true)
                    .build();
                let idx = start + offset;
                let c = ctl.clone();
                row.connect_activated(move |_| c.play_index(idx));
                list.append(&row);
            }
        }
    };
    render();
    ctl.subscribe(move |ev| {
        if matches!(
            ev,
            crate::app::AppEvent::QueueChanged | crate::app::AppEvent::NowPlaying(_)
        ) {
            render();
        }
    });
    root
}

fn is_link(q: &str) -> bool {
    banshee::sources::youtube::parse_video_url(q).is_some()
        || banshee::sources::spotify::track_from_url(q).is_some()
}

fn item_key(i: &SearchItem) -> String {
    match i {
        SearchItem::Track(t) => t.key(),
        SearchItem::Collection(c) => c.key(),
    }
}

fn status(icon: &str, title: &str, desc: &str) -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .description(desc)
        .build()
}

impl SearchPage {
    /// `compact`: the Mini Mode quick-add (no filters, no Home, small type).
    pub fn new(ctl: &Rc<Controller>, compact: bool) -> Rc<Self> {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(if compact {
                "Add to queue…"
            } else {
                "Search songs, videos and podcasts"
            })
            .search_delay(0)
            .hexpand(true)
            .build();
        entry.add_css_class(if compact {
            "search-compact"
        } else {
            "search-hero"
        });
        entry.update_property(&[gtk::accessible::Property::Label("Search")]);

        let spinner = adw::Spinner::builder()
            .visible(false)
            .tooltip_text("Searching")
            .build();
        spinner.set_size_request(24, 24);

        let entry_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        entry_row.append(&entry);
        entry_row.append(&spinner);

        let filters = adw::ToggleGroup::builder()
            .halign(gtk::Align::Center)
            .build();
        filters.add_css_class("round");
        for (name, label) in [
            ("all", "All"),
            ("music", "Music"),
            ("videos", "Videos"),
            ("podcasts", "Podcasts"),
        ] {
            filters.add(adw::Toggle::builder().name(name).label(label).build());
        }
        filters.set_active_name(Some("all"));
        filters.set_visible(!compact);

        let header = gtk::Box::new(gtk::Orientation::Vertical, 12);
        header.append(&entry_row);
        header.append(&filters);
        header.set_margin_top(if compact { 6 } else { 24 });
        header.set_margin_bottom(if compact { 6 } else { 12 });
        header.set_margin_start(12);
        header.set_margin_end(12);
        let header_clamp = adw::Clamp::builder()
            .maximum_size(760)
            .child(&header)
            .build();

        let banner = adw::Banner::builder()
            .button_label("Retry")
            .revealed(false)
            .build();

        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::builder()
            .model(&store)
            .autoselect(true)
            .build();
        let factory = gtk::SignalListItemFactory::new();
        let list = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(false)
            .build();
        list.add_css_class("rich-list");
        list.add_css_class("results");
        list.update_property(&[gtk::accessible::Property::Label("Search results")]);
        let list_clamp = adw::ClampScrollable::builder()
            .maximum_size(760)
            .child(&list)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list_clamp)
            .build();

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();
        let recent = adw::WrapBox::new();
        recent.set_child_spacing(8);
        recent.set_line_spacing(8);
        recent
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Label("Recent searches")]);
        let recent_title = gtk::Label::builder()
            .label("Recent Searches")
            .xalign(0.0)
            .build();
        recent_title.add_css_class("heading");
        let recent_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
        recent_box.append(&recent_title);
        recent_box.append(&recent);
        let recent_clamp = adw::Clamp::builder()
            .maximum_size(if compact { 520 } else { 1100 })
            .child(&recent_box)
            .build();
        let home = (!compact).then(|| crate::ui::home::HomeView::new(ctl));
        let empty_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        match &home {
            // Full window: the empty search is YouTube Music's Home, recent searches on top.
            Some(h) => {
                let body = gtk::Box::new(gtk::Orientation::Vertical, 24);
                body.append(&recent_clamp);
                body.append(&h.root);
                body.set_margin_bottom(24);
                empty_scroller.set_child(Some(&body));
            }
            // Mini Mode quick-add: recent searches and what's coming up in the queue.
            None => {
                let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
                body.set_margin_start(12);
                body.set_margin_end(12);
                body.set_margin_bottom(12);
                let hint = gtk::Label::builder()
                    .label("Type, then press Enter to add the top result. Keep typing to add more.")
                    .wrap(true)
                    .xalign(0.0)
                    .build();
                hint.add_css_class("dim-label");
                body.append(&hint);
                body.append(&recent_clamp);
                body.append(&up_next_box(ctl));
                empty_scroller.set_child(Some(&body));
            }
        }
        stack.add_named(&empty_scroller, Some("empty"));
        stack.add_named(&scroller, Some("results"));
        let searching = status("", "Searching…", "");
        searching.set_paintable(Some(&adw::SpinnerPaintable::new(Some(&searching))));
        stack.add_named(&searching, Some("searching"));
        stack.add_named(
            &status(
                "insert-link-symbolic",
                "Queue This Link",
                "Press Enter to add it to the queue.",
            ),
            Some("link"),
        );
        stack.add_named(
            &status(
                "edit-find-symbolic",
                "No Results",
                "Try different words or another filter.",
            ),
            Some("none"),
        );
        stack.add_named(
            &status("network-error-symbolic", "Search Failed", ""),
            Some("error"),
        );
        stack.set_visible_child_name("empty");

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&banner);
        root.append(&header_clamp);
        root.append(&stack);

        let page = Rc::new(Self {
            root,
            entry,
            ctl: ctl.clone(),
            store,
            selection,
            list,
            stack,
            recent,
            recent_box,
            home,
            compact,
            filters: filters.clone(),
            spinner,
            banner,
            filter: Cell::new(SearchFilter::All),
            generation: Cell::new(0),
            pending: Cell::new(0),
            remote: RefCell::new(HashMap::new()),
            errors: RefCell::new(HashMap::new()),
            debounce: RefCell::new(None),
            rendered_query: RefCell::new(String::new()),
            user_moved: Cell::new(false),
            pending_enter: Cell::new(None),
            settle: RefCell::new(None),
            open_collection: RefCell::new(None),
        });

        // Row factory.
        {
            let ctl_outer = ctl.clone();
            let ctl = ctl_outer.clone();
            let weak_page = Rc::downgrade(&page);
            factory.connect_setup(move |_, item| {
                let Some(li) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let row = ItemRow::new(
                    &ctl,
                    if compact {
                        RowMode::QuickAdd
                    } else {
                        RowMode::Result
                    },
                );
                if let (Some(b), Some(p)) = (row.primary_button(), weak_page.upgrade()) {
                    let wp = Rc::downgrade(&p);
                    b.connect_clicked(move |_| {
                        if let Some(p) = wp.upgrade() {
                            p.ctl.remember_query(&p.query());
                        }
                    });
                }
                li.set_child(Some(&row));
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
                let it = obj.borrow::<SearchItem>().clone();
                row.bind(&ctl, RowItem::Result(it), false);
            });
        }

        let weak = Rc::downgrade(&page);
        page.entry.connect_search_changed(move |_| {
            if let Some(p) = weak.upgrade() {
                p.on_query_changed();
            }
        });
        let weak = Rc::downgrade(&page);
        page.entry.connect_activate(move |_| {
            if let Some(p) = weak.upgrade() {
                p.queue_selected(false);
            }
        });
        let weak = Rc::downgrade(&page);
        page.entry.connect_stop_search(move |e| {
            if e.text().is_empty() {
                return;
            }
            e.set_text("");
            if let Some(p) = weak.upgrade() {
                p.on_query_changed();
            }
        });
        // Arrow keys move the highlighted result while focus stays in the entry;
        // Shift+Enter makes it play next, Ctrl+Enter plays it now.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&page);
        keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(p) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gdk::Key::Down | gdk::Key::Up => {
                    p.move_selection(if key == gdk::Key::Down { 1 } else { -1 });
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter
                    if mods.contains(gdk::ModifierType::SHIFT_MASK) =>
                {
                    p.queue_selected(true);
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter
                    if mods.contains(gdk::ModifierType::CONTROL_MASK) =>
                {
                    p.play_selected();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        page.entry.add_controller(keys);

        let weak = Rc::downgrade(&page);
        page.list.connect_activate(move |_, pos| {
            let Some(p) = weak.upgrade() else { return };
            p.selection.set_selected(pos);
            p.activate_selected();
        });

        let weak = Rc::downgrade(&page);
        filters.connect_active_name_notify(move |g| {
            let Some(p) = weak.upgrade() else { return };
            let f = match g.active_name().as_deref() {
                Some("music") => SearchFilter::Music,
                Some("videos") => SearchFilter::Videos,
                Some("podcasts") => SearchFilter::Podcasts,
                _ => SearchFilter::All,
            };
            p.filter.set(f);
            p.remote.borrow_mut().clear();
            p.on_query_changed();
            p.entry.grab_focus();
        });

        let weak = Rc::downgrade(&page);
        page.banner.connect_button_clicked(move |b| {
            b.set_revealed(false);
            if let Some(p) = weak.upgrade() {
                p.fire_remote(true);
            }
        });

        let weak = Rc::downgrade(&page);
        ctl.subscribe(move |ev| {
            let Some(p) = weak.upgrade() else { return };
            match ev {
                crate::app::AppEvent::AccountsChanged => {
                    p.remote.borrow_mut().clear();
                    p.on_query_changed();
                }
                crate::app::AppEvent::HistoryChanged => p.render_recent(),
                _ => {}
            }
        });
        page.render_recent();
        // Only the main Search page carries the remembered query; Mini's quick-add starts empty.
        if !compact {
            page.restore_last_search();
        }
        page
    }

    /// Bring back the last query, filter and results from the previous session.
    fn restore_last_search(self: &Rc<Self>) {
        let (q, f) = {
            let h = self.ctl.history.borrow();
            (h.last_query.clone(), h.last_filter)
        };
        let name = match f {
            SearchFilter::All => "all",
            SearchFilter::Music => "music",
            SearchFilter::Videos => "videos",
            SearchFilter::Podcasts => "podcasts",
        };
        self.filters.set_active_name(Some(name));
        self.filter.set(f);
        if !q.is_empty() {
            self.entry.set_text(&q);
        }
    }

    fn render_recent(self: &Rc<Self>) {
        self.recent.remove_all();
        let recent = self.ctl.history.borrow().recent.clone();
        self.recent_box.set_visible(!recent.is_empty());
        for q in recent.into_iter().take(if self.compact { 6 } else { 12 }) {
            // A chip: tap to search again, × to forget.
            let label = gtk::Button::builder()
                .label(&q)
                .tooltip_text("Search again")
                .build();
            label.add_css_class("flat");
            label.add_css_class("chip-label");
            let forget = gtk::Button::builder()
                .icon_name("window-close-symbolic")
                .tooltip_text("Remove from Recent Searches")
                .build();
            forget.add_css_class("flat");
            forget.add_css_class("chip-close");
            forget.update_property(&[gtk::accessible::Property::Label(
                "Remove from recent searches",
            )]);
            {
                let (ctl, q) = (self.ctl.clone(), q.clone());
                forget.connect_clicked(move |_| ctl.forget_query(&q));
            }
            let weak = Rc::downgrade(self);
            label.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.entry.set_text(&q);
                    p.entry.grab_focus();
                    p.entry.set_position(-1);
                }
            });
            let chip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            chip.add_css_class("chip");
            chip.append(&label);
            chip.append(&forget);
            self.recent.append(&chip);
        }
    }

    pub fn set_open_collection(&self, f: impl Fn(Collection) + 'static) {
        let f: Rc<dyn Fn(Collection)> = Rc::new(f);
        if let Some(h) = &self.home {
            h.set_open_collection(f.clone());
        }
        *self.open_collection.borrow_mut() = Some(Box::new(move |c| f(c)));
    }

    pub fn focus(&self) {
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    fn query(&self) -> String {
        self.entry.text().trim().to_string()
    }

    fn on_query_changed(self: &Rc<Self>) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        if let Some(id) = self.debounce.borrow_mut().take() {
            id.remove();
        }
        if let Some(id) = self.settle.borrow_mut().take() {
            id.remove();
        }
        let q = self.query();
        if q.is_empty() {
            if !self.compact {
                self.ctl.set_last_search("", self.filter.get());
            }
            self.remote.borrow_mut().clear();
            self.errors.borrow_mut().clear();
            self.pending.set(0);
            self.spinner.set_visible(false);
            self.banner.set_revealed(false);
            self.store.remove_all();
            self.stack.set_visible_child_name("empty");
            return;
        }
        // A pasted link is queued as is on Enter; never searched or remembered.
        if is_link(&q) {
            if !self.compact {
                self.ctl.set_last_search("", self.filter.get());
            }
            self.remote.borrow_mut().clear();
            self.errors.borrow_mut().clear();
            self.pending.set(0);
            self.spinner.set_visible(false);
            self.banner.set_revealed(false);
            self.store.remove_all();
            self.stack.set_visible_child_name("link");
            return;
        }
        // Memoised remote results show instantly.
        let filter = self.filter.get();
        if !self.compact {
            self.ctl.set_last_search(&q, filter);
        }
        // Previous results stay only if they still fuzzy-match the new text, so the list
        // never goes blank between keystrokes but never offers an unrelated item either.
        {
            let mut remote = self.remote.borrow_mut();
            for items in remote.values_mut() {
                *items = self.ctl.still_matching(&q, std::mem::take(items));
            }
        }
        self.errors.borrow_mut().clear();
        // Fresh memo, else results remembered from earlier sessions: both show instantly.
        for s in self.ctl.search_sources() {
            if let Some(items) = self
                .ctl
                .memoised(s, filter, &q)
                .or_else(|| self.ctl.remembered(s, filter, &q))
            {
                self.remote.borrow_mut().insert(s, items);
            }
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            if let Some(p) = weak.upgrade() {
                p.debounce.borrow_mut().take();
                if p.generation.get() == generation {
                    p.fire_remote(false);
                }
            }
        });
        // Store the debounce first so render() shows "Searching…", not "No Results".
        *self.debounce.borrow_mut() = Some(id);
        self.render();
    }

    fn fire_remote(self: &Rc<Self>, force: bool) {
        let q = self.query();
        if q.is_empty() {
            return;
        }
        let generation = self.generation.get();
        let filter = self.filter.get();
        self.banner.set_revealed(false);
        for source in self.ctl.search_sources() {
            if !force && self.ctl.memoised(source, filter, &q).is_some() {
                continue;
            }
            self.pending.set(self.pending.get() + 1);
            self.update_spinner();
            let src = self.ctl.source(source);
            let weak = Rc::downgrade(self);
            let busy = self.ctl.busy_guard();
            let query = q.clone();
            glib::spawn_future_local(async move {
                let r = run(src.search(query.clone(), filter)).await;
                drop(busy);
                let Some(p) = weak.upgrade() else { return };
                p.pending.set(p.pending.get().saturating_sub(1));
                p.update_spinner();
                if p.generation.get() != generation {
                    return; // stale response: the user kept typing
                }
                match r.map_err(SourceError::Unavailable).and_then(|r| r) {
                    Ok(items) => {
                        p.ctl.remember_search(source, filter, &query, &items);
                        let found = !items.is_empty();
                        p.remote.borrow_mut().insert(source, items);
                        p.errors.borrow_mut().remove(&source);
                        if found && !p.compact {
                            p.remember_when_settled(generation, &query);
                        }
                    }
                    Err(e) => {
                        log::warn!("search {source}: {e}");
                        p.errors
                            .borrow_mut()
                            .insert(source, crate::app::describe(source, &e));
                    }
                }
                p.render();
            });
        }
        self.render();
    }

    /// A query whose results the user looked at for a moment counts as a Recent Search,
    /// even if nothing was queued; half-typed prefixes never do.
    fn remember_when_settled(self: &Rc<Self>, generation: u64, query: &str) {
        if self.settle.borrow().is_some() {
            return;
        }
        let (weak, query) = (Rc::downgrade(self), query.to_string());
        let id = glib::timeout_add_local_once(SETTLE, move || {
            if let Some(p) = weak.upgrade() {
                p.settle.borrow_mut().take();
                if p.generation.get() == generation {
                    p.ctl.remember_query(&query);
                }
            }
        });
        *self.settle.borrow_mut() = Some(id);
    }

    fn update_spinner(&self) {
        self.spinner.set_visible(self.pending.get() > 0);
    }

    /// Re-rank local + remote results for the current query and swap them into the list.
    fn render(&self) {
        let q = self.query();
        let filter = self.filter.get();
        let local = self.ctl.search_local(&q, filter);
        let remote: Vec<Vec<SearchItem>> = {
            let r = self.remote.borrow();
            let mut keys: Vec<_> = r.keys().copied().collect();
            keys.sort();
            keys.into_iter()
                .filter_map(|k| r.get(&k).cloned())
                .collect()
        };
        let ranked = self.ctl.rank(&q, filter, &local, &remote);
        let objs: Vec<glib::BoxedAnyObject> = ranked
            .into_iter()
            .map(|r| glib::BoxedAnyObject::new(r.item))
            .collect();
        let n = objs.len();
        // Keep the user's highlight when late results re-render the same query.
        // Late results re-rank the list. Keep the highlight on the same item only if the
        // user put it there; otherwise Enter must always take the current best match.
        let same_query = *self.rendered_query.borrow() == q;
        if !same_query {
            self.user_moved.set(false);
        }
        let keep = (same_query && self.user_moved.get())
            .then(|| self.selected_item().map(|i| item_key(&i)))
            .flatten();
        self.store.splice(0, self.store.n_items(), &objs);
        *self.rendered_query.borrow_mut() = q.clone();
        if n > 0 {
            let pos = keep
                .and_then(|k| {
                    (0..self.store.n_items()).find(|i| {
                        self.store
                            .item(*i)
                            .and_downcast::<glib::BoxedAnyObject>()
                            .is_some_and(|o| item_key(&o.borrow::<SearchItem>()) == k)
                    })
                })
                .unwrap_or(0);
            self.selection.set_selected(pos);
            if pos == 0 {
                self.list.scroll_to(0, gtk::ListScrollFlags::NONE, None);
            }
            // Enter was pressed while nothing had arrived yet: queue the best match now.
            if let Some((g, play_next)) = self.pending_enter.get() {
                if g == self.generation.get() {
                    self.pending_enter.set(None);
                    if let Some(item) = self.selected_item() {
                        self.ctl.remember_query(&q);
                        match item {
                            SearchItem::Track(t) if play_next => self.ctl.play_next(t),
                            SearchItem::Track(t) => self.ctl.enqueue(t),
                            SearchItem::Collection(c) => self.ctl.enqueue_collection(c),
                        }
                        self.entry.select_region(0, -1);
                    }
                }
            }
        }
        let errors = self.errors.borrow();
        if !errors.is_empty() {
            let msg: Vec<&str> = errors.values().map(String::as_str).collect();
            self.banner
                .set_title(&glib::markup_escape_text(&msg.join(" · ")));
            self.banner.set_revealed(true);
        }
        let name = if n > 0 {
            "results"
        } else if self.pending.get() > 0 || self.debounce.borrow().is_some() {
            "searching"
        } else if !errors.is_empty() {
            if let Some(sp) = self
                .stack
                .child_by_name("error")
                .and_downcast::<adw::StatusPage>()
            {
                sp.set_description(Some(&glib::markup_escape_text(
                    &errors.values().cloned().collect::<Vec<_>>().join("\n"),
                )));
            }
            "error"
        } else {
            "none"
        };
        self.stack.set_visible_child_name(name);
    }

    fn move_selection(&self, delta: i32) {
        self.user_moved.set(true);
        let n = self.store.n_items();
        if n == 0 {
            return;
        }
        let cur = self.selection.selected();
        let next = if cur == gtk::INVALID_LIST_POSITION {
            0
        } else {
            (cur as i64 + delta as i64).clamp(0, n as i64 - 1) as u32
        };
        self.selection.set_selected(next);
        self.list.scroll_to(next, gtk::ListScrollFlags::NONE, None);
    }

    fn selected_item(&self) -> Option<SearchItem> {
        let obj = self
            .selection
            .selected_item()
            .and_downcast::<glib::BoxedAnyObject>()?;
        Some(obj.borrow::<SearchItem>().clone())
    }

    /// Enter: queue the highlighted result, then select the entry text so the next
    /// keystroke starts the next search.
    fn queue_selected(self: &Rc<Self>, play_next: bool) {
        // A pasted YouTube / YouTube Music / Spotify link is queued as is.
        let q = self.query();
        if is_link(&q) {
            self.ctl.open_uri(&q);
            self.entry.select_region(0, -1);
            return;
        }
        // A fast typist can press Enter before the list caught up with the last keystroke:
        // rank for the current text now, so Enter never takes a row from the previous query.
        if *self.rendered_query.borrow() != q {
            self.on_query_changed();
        }
        let Some(item) = self.selected_item() else {
            // Nothing yet (the network is still answering): take the first result that lands.
            if !q.is_empty() {
                self.pending_enter
                    .set(Some((self.generation.get(), play_next)));
            }
            return;
        };
        self.pending_enter.set(None);
        log::debug!(
            "Enter on “{}” queues {:?} (row {} of {})",
            self.query(),
            match &item {
                SearchItem::Track(t) => t.title.as_str(),
                SearchItem::Collection(c) => c.title.as_str(),
            },
            self.selection.selected(),
            self.store.n_items()
        );
        self.ctl.remember_query(&self.query());
        match item {
            SearchItem::Track(t) if play_next => self.ctl.play_next(t),
            SearchItem::Track(t) => self.ctl.enqueue(t),
            SearchItem::Collection(c) => self.ctl.enqueue_collection(c),
        }
        self.entry.select_region(0, -1);
    }

    fn play_selected(&self) {
        if let Some(SearchItem::Track(t)) = self.selected_item() {
            self.ctl.remember_query(&self.query());
            self.ctl.play_now(t);
            self.entry.select_region(0, -1);
        }
    }

    /// Row activation: tracks are queued (the default action), collections open.
    fn activate_selected(&self) {
        self.ctl.remember_query(&self.query());
        match self.selected_item() {
            Some(SearchItem::Track(t)) => self.ctl.enqueue(t),
            Some(SearchItem::Collection(c)) => {
                if let Some(f) = self.open_collection.borrow().as_ref() {
                    f(c);
                }
            }
            None => {}
        }
    }
}
