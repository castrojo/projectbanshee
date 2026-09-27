//! Touch Mode (ADR 0015): a fullscreen stage lit by the cover, and a Queue built for fingers.
//!
//! Every queue change re-splices the list store, so rows are rebound constantly. Reorder and
//! swipe therefore run from one gesture on the list view and track the entry by id; a row
//! widget never owns drag state.

use crate::app::{AppEvent, Controller};
use crate::ui::player_widgets::{Extras, Progress, TrackInfo, Transport};
use crate::ui::rows::{ItemRow, RowItem, RowMode, keep_going_section};
use crate::ui::search::SearchPage;
use adw::prelude::*;
use adw::subclass::prelude::*;
use banshee::queue::{Advance, EntryId, QueueEntry};
use banshee::touch::{
    DragIntent, Skip, SwipeEnd, autoscroll_step, cover_swipe, drag_intent, swipe_end,
};
use gtk::{gio, glib, graphene, gsk};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SwipeRow {
        pub offset: Cell<f64>,
        pub underlay: RefCell<Option<gtk::Box>>,
        pub row: RefCell<Option<ItemRow>>,
        pub animation: RefCell<Option<adw::Animation>>,
        /// What the running animation commits when it ends (a swipe-remove).
        pub on_done: RefCell<Option<Box<dyn FnOnce()>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SwipeRow {
        const NAME: &'static str = "BansheeSwipeRow";
        type Type = super::SwipeRow;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SwipeRow {
        fn dispose(&self) {
            if let Some(u) = self.underlay.take() {
                u.unparent();
            }
            if let Some(r) = self.row.take() {
                r.unparent();
            }
        }
    }

    impl WidgetImpl for SwipeRow {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let row = self
                .row
                .borrow()
                .as_ref()
                .map(|r| r.measure(orientation, for_size));
            let under = self
                .underlay
                .borrow()
                .as_ref()
                .map(|u| u.measure(orientation, for_size));
            match (row, under) {
                (Some((min, nat, _, _)), Some((umin, unat, _, _))) => {
                    (min.max(umin), nat.max(unat), -1, -1)
                }
                (Some(m), None) | (None, Some(m)) => m,
                (None, None) => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(u) = self.underlay.borrow().as_ref() {
                u.allocate(width, height, baseline, None);
            }
            if let Some(r) = self.row.borrow().as_ref() {
                let shift = graphene::Point::new(self.offset.get() as f32, 0.0);
                r.allocate(
                    width,
                    height,
                    baseline,
                    Some(gsk::Transform::new().translate(&shift)),
                );
            }
        }
    }
}

glib::wrapper! {
    /// A queue row that can slide sideways over a red "remove" underlay.
    pub struct SwipeRow(ObjectSubclass<imp::SwipeRow>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SwipeRow {
    fn new(ctl: &Rc<Controller>) -> Self {
        let obj: Self = glib::Object::new();
        obj.set_overflow(gtk::Overflow::Hidden);
        obj.add_css_class("touch-row");

        let underlay = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        underlay.add_css_class("swipe-underlay");
        let start = gtk::Image::from_icon_name("user-trash-symbolic");
        let end = gtk::Image::from_icon_name("user-trash-symbolic");
        end.set_hexpand(true);
        end.set_halign(gtk::Align::End);
        underlay.append(&start);
        underlay.append(&end);
        underlay.set_opacity(0.0);
        underlay.set_parent(&obj);

        let row = ItemRow::new(ctl, RowMode::Queue);
        // Text room: long-press opens the same menu; − and swiping remove.
        row.hide_menu_button();
        let grip = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        grip.add_css_class("drag-handle");
        grip.set_tooltip_text(Some("Drag to Reorder"));
        row.append(&grip);
        row.set_parent(&obj);

        *obj.imp().underlay.borrow_mut() = Some(underlay);
        *obj.imp().row.borrow_mut() = Some(row);
        obj
    }

    fn item_row(&self) -> ItemRow {
        self.imp().row.borrow().clone().expect("row is set in new")
    }

    fn entry(&self) -> Option<QueueEntry> {
        match self.item_row().item()? {
            RowItem::Queue { entry, .. } => Some(entry),
            RowItem::Result(_) => None,
        }
    }
    /// Show `entry` at `index`, with the current / played / lifted styling.
    fn show(&self, ctl: &Rc<Controller>, entry: QueueEntry, index: usize, lifted: Option<EntryId>) {
        let current = ctl.current_entry().is_some_and(|c| c.id == entry.id);
        let played = ctl.current_index().is_some_and(|c| index < c);
        let lifted = lifted == Some(entry.id);
        self.item_row()
            .bind(ctl, RowItem::Queue { entry, index }, current);
        for (class, on) in [("current", current), ("played", played), ("lifted", lifted)] {
            if on {
                self.add_css_class(class);
            } else {
                self.remove_css_class(class);
            }
        }
    }

    fn offset(&self) -> f64 {
        self.imp().offset.get()
    }

    fn set_offset(&self, x: f64) {
        self.imp().offset.set(x);
        if let Some(u) = self.imp().underlay.borrow().as_ref() {
            u.set_opacity((x.abs() / 48.0).min(1.0));
        }
        self.queue_allocate();
    }

    /// Back to rest, instantly (a rebind shows another entry). A swipe-remove already decided
    /// still happens, just without the rest of its slide.
    fn reset(&self) {
        if let Some(a) = self.imp().animation.take() {
            a.pause();
        }
        if let Some(commit) = self.imp().on_done.take() {
            // Not now: this runs inside a list rebind, i.e. inside the store splice.
            glib::idle_add_local_once(commit);
        }
        self.set_offset(0.0);
    }

    fn animate(&self, a: impl IsA<adw::Animation>, on_done: Option<Box<dyn FnOnce()>>) {
        let a = a.upcast();
        if let Some(old) = self.imp().animation.replace(Some(a.clone())) {
            old.pause();
        }
        if let Some(commit) = self.imp().on_done.replace(on_done) {
            glib::idle_add_local_once(commit);
        }
        let weak = self.downgrade();
        a.connect_done(move |_| {
            if let Some(row) = weak.upgrade() {
                row.imp().animation.take();
                if let Some(commit) = row.imp().on_done.take() {
                    commit();
                }
            }
        });
        a.play();
    }
}

/// Find the queue row under a picked widget, and whether the pick was on its grip.
fn row_at(picked: Option<gtk::Widget>) -> Option<(SwipeRow, bool)> {
    let mut on_grip = false;
    let mut w = picked;
    while let Some(widget) = w {
        if widget.has_css_class("drag-handle") {
            on_grip = true;
        }
        if let Some(row) = widget.downcast_ref::<SwipeRow>() {
            return Some((row.clone(), on_grip));
        }
        w = widget.parent();
    }
    None
}

fn index_of(store: &gio::ListStore, id: EntryId) -> Option<usize> {
    (0..store.n_items()).find_map(|i| {
        let obj = store.item(i).and_downcast::<glib::BoxedAnyObject>()?;
        (obj.borrow::<QueueEntry>().id == id).then_some(i as usize)
    })
}

enum Drag {
    Idle,
    /// Pressed on a row body; not yet a swipe or a scroll.
    Pending {
        row: SwipeRow,
        id: EntryId,
    },
    /// Pressed on the grip: the entry follows the finger.
    Reorder {
        id: EntryId,
        y: f64,
        /// Where the entry was when the drag began, for cancelling.
        origin: usize,
    },
    Swipe {
        row: SwipeRow,
        id: EntryId,
        last: (Instant, f64),
        velocity: f64,
    },
}

pub struct TouchMode {
    pub root: adw::ToastOverlay,
    sheet: adw::BottomSheet,
    search: Rc<SearchPage>,
    /// The queue's reorder/swipe gesture, so Escape can cancel it.
    queue_gesture: gtk::GestureDrag,
}

impl TouchMode {
    pub fn new(ctl: &Rc<Controller>) -> Rc<Self> {
        // ---- Stage: the cover, big and swipeable, over big controls.
        let info = TrackInfo::new(ctl, 280, true, true);
        info.stack_vertically(24);
        info.root.add_css_class("touch-track");
        let progress = Progress::new(ctl, true);
        progress.root.add_css_class("touch-progress");
        let transport = Transport::new(ctl, 96);
        transport.root.add_css_class("touch-transport");
        transport.root.set_spacing(28);
        let extras = Extras::new(ctl);
        extras.root.add_css_class("touch-extras");
        extras.root.set_halign(gtk::Align::Center);
        extras.root.set_spacing(16);

        let stage = gtk::Box::new(gtk::Orientation::Vertical, 20);
        stage.append(&info.root);
        stage.append(&progress.root);
        stage.append(&transport.root);
        stage.append(&extras.root);
        let stage_clamp = adw::Clamp::builder()
            .maximum_size(560)
            .child(&stage)
            .valign(gtk::Align::Center)
            .margin_start(32)
            .margin_end(32)
            .margin_top(32)
            .margin_bottom(32)
            .build();
        stage_clamp.add_css_class("touch-stage");
        attach_cover_swipe(ctl, &info.root);

        // ---- Queue.
        let (queue, open_add, queue_gesture) = build_queue(ctl);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content.set_homogeneous(true);
        content.append(&stage_clamp);
        content.append(&queue);

        let scrim = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scrim.add_css_class("touch-scrim");
        let leave = gtk::Button::builder()
            .icon_name("view-restore-symbolic")
            .tooltip_text("Leave Touch Mode")
            .action_name("win.touch-mode")
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .margin_start(24)
            .margin_top(24)
            .build();
        leave.add_css_class("circular");
        leave.add_css_class("osd");
        leave.add_css_class("touch-leave");
        leave.update_property(&[gtk::accessible::Property::Label("Leave Touch Mode")]);

        let lit = gtk::Overlay::builder().child(&info.backdrop).build();
        lit.add_overlay(&scrim);
        lit.add_overlay(&content);
        lit.add_overlay(&leave);
        lit.set_measure_overlay(&content, true);
        lit.add_css_class("touch-content");

        // ---- Add: the compact search in a bottom sheet.
        let search = SearchPage::new(ctl, true);
        search.root.add_css_class("touch-search");
        search.root.set_size_request(-1, 440);
        let sheet = adw::BottomSheet::builder()
            .content(&lit)
            .sheet(&search.root)
            .build();

        let bin = adw::BreakpointBin::builder()
            .child(&sheet)
            .width_request(360)
            .height_request(294)
            .build();
        let cover = |bp: &adw::Breakpoint, px: i32| {
            for w in info.art.size_widgets() {
                bp.add_setter(&w, "width-request", Some(&px.to_value()));
                bp.add_setter(&w, "height-request", Some(&px.to_value()));
            }
        };
        // Short landscape (2-in-1s at 125–150 %): a smaller cover, tighter stage.
        let short = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxHeight,
            760.0,
            adw::LengthUnit::Sp,
        ));
        cover(&short, 200);
        short.add_setter(&stage, "spacing", Some(&12.to_value()));
        short.add_setter(&stage_clamp, "margin-top", Some(&16.to_value()));
        short.add_setter(&stage_clamp, "margin-bottom", Some(&16.to_value()));
        bin.add_breakpoint(short);
        // Portrait, or too narrow for two columns (split screen): a compact stage — cover
        // beside the text, no extras — above the queue, which gets most of the screen.
        // Added last so it wins when both match.
        let stacked = adw::Breakpoint::new(adw::BreakpointCondition::new_or(
            adw::BreakpointCondition::new_ratio(
                adw::BreakpointConditionRatioType::MaxAspectRatio,
                1,
                1,
            ),
            adw::BreakpointCondition::new_length(
                adw::BreakpointConditionLengthType::MaxWidth,
                860.0,
                adw::LengthUnit::Sp,
            ),
        ));
        stacked.add_setter(
            &content,
            "orientation",
            Some(&gtk::Orientation::Vertical.to_value()),
        );
        stacked.add_setter(&content, "homogeneous", Some(&false.to_value()));
        stacked.add_setter(&queue, "vexpand", Some(&true.to_value()));
        stacked.add_setter(&queue, "margin-start", Some(&24.to_value()));
        stacked.add_setter(&stage_clamp, "margin-top", Some(&56.to_value()));
        stacked.add_setter(
            &stage_clamp,
            "css-classes",
            Some(&["touch-stage", "compact"][..].to_value()),
        );
        stacked.add_setter(&stage_clamp, "margin-bottom", Some(&0.to_value()));
        stacked.add_setter(&stage, "spacing", Some(&12.to_value()));
        stacked.add_setter(
            &info.root,
            "orientation",
            Some(&gtk::Orientation::Horizontal.to_value()),
        );
        for label in info.labels() {
            stacked.add_setter(&label, "xalign", Some(&0.0f32.to_value()));
            stacked.add_setter(&label, "lines", Some(&1.to_value()));
            stacked.add_setter(
                &label,
                "justify",
                Some(&gtk::Justification::Left.to_value()),
            );
        }
        stacked.add_setter(&extras.root, "visible", Some(&false.to_value()));
        cover(&stacked, 112);
        bin.add_breakpoint(stacked);

        let root = adw::ToastOverlay::new();
        root.set_child(Some(&bin));
        root.add_css_class("touch-mode");

        let this = Rc::new(Self {
            root,
            sheet,
            search,
            queue_gesture,
        });

        {
            let weak = Rc::downgrade(&this);
            *open_add.borrow_mut() = Some(Rc::new(move || {
                if let Some(t) = weak.upgrade() {
                    t.open_search();
                }
            }));
        }
        {
            let weak = Rc::downgrade(&this);
            this.sheet.connect_open_notify(move |s| {
                if s.is_open() {
                    if let Some(t) = weak.upgrade() {
                        t.search.focus();
                    }
                }
            });
        }
        {
            // Escape, in order: cancel a drag in progress (HIG), close the search sheet
            // (before its entry turns Escape into stop-search), leave Touch Mode.
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = Rc::downgrade(&this);
            keys.connect_key_pressed(move |_, key, _, _| {
                let Some(t) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if key != gtk::gdk::Key::Escape {
                    return glib::Propagation::Proceed;
                }
                if t.queue_gesture.is_active() {
                    t.queue_gesture.reset();
                } else if t.sheet.is_open() {
                    t.sheet.set_open(false);
                } else {
                    let _ = t.root.activate_action("win.touch-mode", None);
                }
                glib::Propagation::Stop
            });
            this.root.add_controller(keys);
        }
        this
    }

    pub fn open_search(&self) {
        self.sheet.set_open(true);
        self.search.focus();
    }

    pub fn close_search(&self) {
        self.sheet.set_open(false);
    }
}

/// Fling the cover sideways to skip; a tap still opens the artist.
fn attach_cover_swipe(ctl: &Rc<Controller>, cover: &gtk::Box) {
    let drag = gtk::GestureDrag::new();
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    drag.connect_drag_update(|g, dx, dy| {
        if drag_intent(dx, dy) == DragIntent::Swipe {
            g.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    let swipe = gtk::GestureSwipe::new();
    swipe.set_propagation_phase(gtk::PropagationPhase::Capture);
    swipe.group_with(&drag);
    let ctl = ctl.clone();
    swipe.connect_swipe(move |_, vx, vy| match cover_swipe(vx, vy) {
        Some(Skip::Next) => ctl.advance(Advance::User),
        Some(Skip::Previous) => ctl.previous(),
        None => {}
    });
    cover.add_controller(drag);
    cover.add_controller(swipe);
}

/// Filled in by the owner once it exists: "open the search".
type OpenAdd = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// The queue slab: header, touch list, empty state. Returns the slab and the Add hook.
fn build_queue(ctl: &Rc<Controller>) -> (gtk::Box, OpenAdd, gtk::GestureDrag) {
    let open_add: OpenAdd = Rc::default();
    let run_add = {
        let open_add = open_add.clone();
        move || {
            let f = open_add.borrow().clone();
            if let Some(f) = f {
                f();
            }
        }
    };

    let title = gtk::Label::builder().label("Queue").xalign(0.0).build();
    title.add_css_class("touch-queue-title");
    let summary = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    summary.add_css_class("touch-queue-summary");
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
    heading.set_hexpand(true);
    heading.set_valign(gtk::Align::Center);
    heading.append(&title);
    heading.append(&summary);

    let add_button = || {
        let b = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("list-add-symbolic")
                    .label("Add")
                    .build(),
            )
            .valign(gtk::Align::Center)
            .build();
        b.add_css_class("pill");
        b.add_css_class("touch-add");
        let run_add = run_add.clone();
        b.connect_clicked(move |_| run_add());
        b
    };
    let menu = gio::Menu::new();
    menu.append(Some("Clear Queue"), Some("win.clear-queue"));
    let menu_btn = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("Queue Menu")
        .menu_model(&menu)
        .valign(gtk::Align::Center)
        .build();
    menu_btn.add_css_class("circular");
    menu_btn.add_css_class("flat");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("touch-queue-header");
    header.append(&heading);
    header.append(&add_button());
    header.append(&menu_btn);

    let dragging: Rc<Cell<Option<EntryId>>> = Rc::default();
    let store = ctl.queue_store.clone();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    // When the user last scrolled the list with wheel, touchpad or finger. Only real input
    // counts: the list also moves itself on relayout, and that isn't browsing.
    let browsed: Rc<Cell<Option<Instant>>> = Rc::default();
    {
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let browsed = browsed.clone();
        wheel.connect_scroll(move |_, _, _| {
            browsed.set(Some(Instant::now()));
            glib::Propagation::Proceed
        });
        scroller.add_controller(wheel);
    }
    crate::ui::hold_scroll_on_removal(&store, &scroller);
    let selection = gtk::NoSelection::new(Some(store.clone()));
    let factory = gtk::SignalListItemFactory::new();
    // Every row this list creates, so the current marker and played dimming can move without
    // a rebind (the list keeps its tiles when the same items are re-announced).
    let rows: Rc<RefCell<Vec<glib::WeakRef<SwipeRow>>>> = Rc::default();
    {
        let (ctl, rows) = (ctl.clone(), rows.clone());
        factory.connect_setup(move |_, item| {
            if let Some(li) = item.downcast_ref::<gtk::ListItem>() {
                let row = SwipeRow::new(&ctl);
                rows.borrow_mut().push(row.downgrade());
                li.set_child(Some(&row));
            }
        });
    }
    {
        let (ctl, dragging) = (ctl.clone(), dragging.clone());
        factory.connect_bind(move |_, item| {
            let Some(li) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(row), Some(obj)) = (
                li.child().and_downcast::<SwipeRow>(),
                li.item().and_downcast::<glib::BoxedAnyObject>(),
            ) else {
                return;
            };
            let entry = obj.borrow::<QueueEntry>().clone();
            row.reset();
            row.show(&ctl, entry, li.position() as usize, dragging.get());
        });
    }
    let restyle: Rc<dyn Fn()> = {
        let (ctl, rows, dragging) = (ctl.clone(), rows.clone(), dragging.clone());
        Rc::new(move || {
            rows.borrow_mut().retain(|w| w.upgrade().is_some());
            for row in rows.borrow().iter().filter_map(glib::WeakRef::upgrade) {
                // Resolve the position now: rows outside a change keep their bound index.
                if let Some(entry) = row.entry() {
                    if let Some(index) = ctl.entry_index(entry.id) {
                        row.show(&ctl, entry, index, dragging.get());
                    }
                }
            }
        })
    };
    let list = gtk::ListView::builder()
        .model(&selection)
        .factory(&factory)
        .single_click_activate(true)
        .build();
    list.add_css_class("touch-queue-list");
    list.update_property(&[gtk::accessible::Property::Label("Queue")]);
    {
        let ctl = ctl.clone();
        list.connect_activate(move |_, pos| ctl.play_index(pos as usize));
    }
    {
        // Open on what's playing, not on the played entries above it (after the first layout,
        // or the list ignores it).
        let (ctl, scroller) = (ctl.clone(), scroller.clone());
        list.connect_map(move |_| {
            let (ctl, scroller) = (ctl.clone(), scroller.clone());
            glib::idle_add_local_once(move || show_current(&ctl, &scroller));
        });
    }
    scroller.set_child(Some(&list));
    let gesture = attach_queue_gestures(ctl, &list, &scroller, &dragging, &restyle, &browsed);

    let empty_add = add_button();
    empty_add.add_css_class("suggested-action");
    empty_add.set_halign(gtk::Align::Center);
    let empty = adw::StatusPage::builder()
        .icon_name("view-list-symbolic")
        .title("Queue Is Empty")
        .description("Add songs, videos and podcasts, in any order.")
        .child(&empty_add)
        .build();
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&scroller, Some("list"));

    let slab = gtk::Box::new(gtk::Orientation::Vertical, 0);
    slab.add_css_class("touch-queue");
    slab.set_margin_top(24);
    slab.set_margin_bottom(24);
    slab.set_margin_end(24);
    slab.append(&header);
    slab.append(&stack);
    // Keep Going under the list when nothing is Up next: touch rows, a few visible at once.
    slab.append(&keep_going_section(ctl, Some("touch-row"), Some(250)));

    let refresh = {
        let (ctl, stack, summary) = (ctl.clone(), stack.clone(), summary.clone());
        move || {
            let n = ctl.queue_len();
            stack.set_visible_child_name(if n == 0 { "empty" } else { "list" });
            summary.set_label(&crate::ui::queue_panel::queue_summary(&ctl));
        }
    };
    refresh();
    {
        let weak = Rc::downgrade(ctl);
        let scroller = scroller.clone();
        ctl.subscribe(move |ev| match ev {
            AppEvent::QueueChanged => refresh(),
            AppEvent::NowPlaying(_) => {
                refresh();
                // Move the current marker and played dimming, then follow what's playing,
                // unless a finger is reordering or the user is browsing the list (a track
                // ending must not move the rows they're reaching for).
                restyle();
                let browsing = browsed
                    .get()
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(10));
                if dragging.get().is_none() && !browsing {
                    if let Some(c) = weak.upgrade() {
                        show_current(&c, &scroller);
                    }
                }
            }
            _ => {}
        });
    }
    (slab, open_add, gesture)
}

/// Put the playing entry at the top of the list, with what's up next below it.
fn show_current(ctl: &Controller, scroller: &gtk::ScrolledWindow) {
    let (Some(i), n) = (ctl.current_index(), ctl.queue_len()) else {
        return;
    };
    let adj = scroller.vadjustment();
    if n == 0 || adj.upper() <= adj.page_size() {
        return;
    }
    // Rows are one height, so the list's extent divides evenly; keep a sliver of the
    // previous entry visible for context.
    let row = adj.upper() / n as f64;
    let max = adj.upper() - adj.page_size();
    adj.set_value(((i as f64 - 0.25) * row).clamp(0.0, max));
}

/// One gesture on the list: grip → live reorder (with edge auto-scroll), sideways → swipe to
/// remove, anything else → scrolling and taps as usual.
fn attach_queue_gestures(
    ctl: &Rc<Controller>,
    list: &gtk::ListView,
    scroller: &gtk::ScrolledWindow,
    dragging: &Rc<Cell<Option<EntryId>>>,
    restyle: &Rc<dyn Fn()>,
    browsed: &Rc<Cell<Option<Instant>>>,
) -> gtk::GestureDrag {
    let state: Rc<RefCell<Drag>> = Rc::new(RefCell::new(Drag::Idle));
    let start: Rc<Cell<(f64, f64)>> = Rc::default();
    let store = ctl.queue_store.clone();

    // Move the dragged entry to the row under `y` (list coordinates).
    let reorder_to: Rc<dyn Fn(f64)> = {
        let (ctl, list, state, start, store) = (
            ctl.clone(),
            list.clone(),
            state.clone(),
            start.clone(),
            store.clone(),
        );
        Rc::new(move |y| {
            let Drag::Reorder { id, .. } = *state.borrow() else {
                return;
            };
            let y = y.clamp(1.0, (list.height() - 1).max(1) as f64);
            let x = start.get().0;
            let Some((target, _)) = row_at(list.pick(x, y, gtk::PickFlags::DEFAULT)) else {
                return;
            };
            let Some(to) = target.entry().and_then(|e| index_of(&store, e.id)) else {
                return;
            };
            if let Some(from) = index_of(&store, id) {
                if from != to {
                    ctl.move_entry(from, to);
                }
            }
        })
    };

    let gesture = gtk::GestureDrag::new();
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (ctl, list, state, start, dragging, reorder_to, scroller) = (
            ctl.clone(),
            list.clone(),
            state.clone(),
            start.clone(),
            dragging.clone(),
            reorder_to.clone(),
            scroller.clone(),
        );
        // Each grip drag gets its own auto-scroll ticker; older ones retire.
        let ticker = Rc::new(Cell::new(0u64));
        gesture.connect_drag_begin(move |g, x, y| {
            start.set((x, y));
            let Some((row, on_grip)) = row_at(list.pick(x, y, gtk::PickFlags::DEFAULT)) else {
                *state.borrow_mut() = Drag::Idle;
                return;
            };
            let Some(entry) = row.entry() else { return };
            if !on_grip {
                *state.borrow_mut() = Drag::Pending { row, id: entry.id };
                return;
            }
            let Some(origin) = ctl.entry_index(entry.id) else {
                return;
            };
            g.set_state(gtk::EventSequenceState::Claimed);
            *state.borrow_mut() = Drag::Reorder {
                id: entry.id,
                y,
                origin,
            };
            dragging.set(Some(entry.id));
            row.add_css_class("lifted");
            // Auto-scroll while the finger rests near an edge.
            let generation = ticker.get() + 1;
            ticker.set(generation);
            let (list, state, reorder_to, scroller, ticker) = (
                list.clone(),
                state.clone(),
                reorder_to.clone(),
                scroller.clone(),
                ticker.clone(),
            );
            glib::timeout_add_local(Duration::from_millis(16), move || {
                let Drag::Reorder { y, .. } = *state.borrow() else {
                    return glib::ControlFlow::Break;
                };
                if ticker.get() != generation {
                    return glib::ControlFlow::Break;
                }
                let step = autoscroll_step(y, list.height() as f64);
                if step != 0.0 {
                    let adj = scroller.vadjustment();
                    let max = adj.upper() - adj.page_size();
                    adj.set_value((adj.value() + step).clamp(adj.lower(), max));
                    reorder_to(y);
                }
                glib::ControlFlow::Continue
            });
        });
    }
    {
        let (state, start, reorder_to, browsed) = (
            state.clone(),
            start.clone(),
            reorder_to.clone(),
            browsed.clone(),
        );
        gesture.connect_drag_update(move |g, dx, dy| {
            let mut s = state.borrow_mut();
            match &mut *s {
                Drag::Reorder { y, .. } => {
                    *y = start.get().1 + dy;
                    let y = *y;
                    drop(s);
                    reorder_to(y);
                }
                Drag::Pending { row, id } => match drag_intent(dx, dy) {
                    DragIntent::Swipe => {
                        g.set_state(gtk::EventSequenceState::Claimed);
                        row.set_offset(dx);
                        *s = Drag::Swipe {
                            row: row.clone(),
                            id: *id,
                            last: (Instant::now(), dx),
                            velocity: 0.0,
                        };
                    }
                    DragIntent::Scroll => {
                        // A finger panning the list: browsing.
                        browsed.set(Some(Instant::now()));
                        g.set_state(gtk::EventSequenceState::Denied);
                        *s = Drag::Idle;
                    }
                    DragIntent::Undecided => {}
                },
                Drag::Swipe {
                    row,
                    last,
                    velocity,
                    ..
                } => {
                    let now = Instant::now();
                    let dt = now.duration_since(last.0).as_secs_f64();
                    if dt > 0.004 {
                        // Lightly smoothed px/s, so one jittery frame can't fake a fling.
                        let v = (dx - last.1) / dt;
                        *velocity = 0.6 * v + 0.4 * *velocity;
                        *last = (now, dx);
                    }
                    row.set_offset(dx);
                }
                Drag::Idle => {}
            }
        });
    }
    {
        let (ctl, state, dragging, restyle) = (
            ctl.clone(),
            state.clone(),
            dragging.clone(),
            restyle.clone(),
        );
        gesture.connect_drag_end(move |_, _, _| {
            // End the borrow before the arms run: they re-enter list binds.
            let prev = std::mem::replace(&mut *state.borrow_mut(), Drag::Idle);
            match prev {
                Drag::Reorder { .. } => {
                    dragging.set(None);
                    // Drop the lifted style wherever the entry landed.
                    restyle();
                }
                Drag::Swipe {
                    row, id, velocity, ..
                } => finish_swipe(&ctl, &row, id, velocity),
                Drag::Pending { .. } | Drag::Idle => {}
            }
        });
    }
    {
        // A cancelled drag (Escape, a grab, the sequence claimed elsewhere) never commits:
        // a swipe springs back and a reorder puts the entry back where it started. `cancel`
        // comes before `drag-end`, which then finds nothing to do.
        let (ctl, state, dragging, restyle) = (
            ctl.clone(),
            state.clone(),
            dragging.clone(),
            restyle.clone(),
        );
        gesture.connect_cancel(move |_, _| {
            let prev = std::mem::replace(&mut *state.borrow_mut(), Drag::Idle);
            match prev {
                Drag::Reorder { id, origin, .. } => {
                    dragging.set(None);
                    if let Some(now) = ctl.entry_index(id).filter(|&now| now != origin) {
                        ctl.move_entry(now, origin);
                    }
                    restyle();
                }
                Drag::Swipe { row, velocity, .. } => spring_back(&row, velocity),
                Drag::Pending { .. } | Drag::Idle => {}
            }
        });
    }
    list.add_controller(gesture.clone());
    gesture
}

/// Slide the row out and remove its entry, or spring it back.
fn finish_swipe(ctl: &Rc<Controller>, row: &SwipeRow, id: EntryId, velocity: f64) {
    let width = f64::from(row.width());
    let from = row.offset();
    match swipe_end(from, width, velocity) {
        SwipeEnd::Remove => {
            let to = width.copysign(if from == 0.0 { velocity } else { from });
            let a = adw::TimedAnimation::new(row, from, to, 180, offset_target(row));
            a.set_easing(adw::Easing::EaseOutCubic);
            let ctl = ctl.clone();
            row.animate(a, Some(Box::new(move || ctl.remove_entry(id, true))));
        }
        SwipeEnd::Restore => spring_back(row, velocity),
    }
}

/// Spring the row back to rest from wherever the finger left it.
fn spring_back(row: &SwipeRow, velocity: f64) {
    let params = adw::SpringParams::new(0.9, 1.0, 500.0);
    let a = adw::SpringAnimation::new(row, row.offset(), 0.0, params, offset_target(row));
    a.set_initial_velocity(velocity);
    a.set_clamp(false);
    row.animate(a, None);
}

/// Animates the row's offset without keeping the row alive (it owns the animation).
fn offset_target(row: &SwipeRow) -> adw::CallbackAnimationTarget {
    let row = row.downgrade();
    adw::CallbackAnimationTarget::new(move |v| {
        if let Some(row) = row.upgrade() {
            row.set_offset(v);
        }
    })
}
