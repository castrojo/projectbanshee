//! Queue sidebar: always-visible, editable independently of playback (ADR 0007).

use crate::app::{AppEvent, Controller};
use crate::ui::rows::{ItemRow, RowItem, RowMode};
use adw::prelude::*;
use banshee::queue::QueueEntry;
use gtk::{gdk, gio, glib};
use std::rc::Rc;

pub struct QueuePanel {
    pub root: adw::ToolbarView,
    pub header: adw::HeaderBar,
}

impl QueuePanel {
    pub fn new(ctl: &Rc<Controller>, window: &adw::ApplicationWindow) -> Rc<Self> {
        let title = adw::WindowTitle::new("Queue", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();

        let menu = gio::Menu::new();
        menu.append(Some("Clear Queue"), Some("win.clear-queue"));
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("Queue Menu")
            .menu_model(&menu)
            .build();
        header.pack_end(&menu_btn);

        let selection = gtk::NoSelection::new(Some(ctl.queue_store.clone()));
        let factory = gtk::SignalListItemFactory::new();
        let list = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(false)
            .build();
        list.add_css_class("navigation-sidebar");
        list.add_css_class("queue-list");
        list.update_property(&[gtk::accessible::Property::Label("Queue")]);

        {
            let ctl_outer = ctl.clone();
            let ctl = ctl_outer.clone();
            factory.connect_setup(move |_, item| {
                let Some(li) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let row = ItemRow::new(&ctl, RowMode::Queue);
                attach_dnd(&ctl, &row);
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
                let entry = obj.borrow::<QueueEntry>().clone();
                let index = li.position() as usize;
                let current = ctl.current_entry().is_some_and(|c| c.id == entry.id);
                row.bind(&ctl, RowItem::Queue { entry, index }, current);
                // Entries before the current one have played: dim them.
                let played = ctl.current_index().is_some_and(|c| index < c);
                if played {
                    row.add_css_class("played");
                } else {
                    row.remove_css_class("played");
                }
            });
        }
        {
            let ctl = ctl.clone();
            list.connect_activate(move |_, pos| ctl.play_index(pos as usize));
        }

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("view-list-symbolic")
            .title("Queue Is Empty")
            .description("Search, then press Enter or + to add items. Music, videos and podcasts can be mixed freely.")
            .build();
        empty.add_css_class("compact");
        let stack = gtk::Stack::new();
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scroller, Some("list"));

        let root = adw::ToolbarView::builder().content(&stack).build();
        root.add_top_bar(&header);

        let refresh = {
            let (ctl, stack, title) = (ctl.clone(), stack.clone(), title.clone());
            move || {
                let n = ctl.queue_len();
                stack.set_visible_child_name(if n == 0 { "empty" } else { "list" });
                // What's still to come matters more than the whole list.
                let start = ctl.current_index().map_or(0, |c| c + 1);
                let upcoming: Vec<_> = ctl.queue_tracks().into_iter().skip(start).collect();
                let left: u64 = upcoming
                    .iter()
                    .filter_map(|t| t.duration_secs)
                    .map(u64::from)
                    .sum();
                let sub = match upcoming.len() {
                    0 if n > 0 => "Nothing up next".to_string(),
                    0 => String::new(),
                    1 => "1 up next".into(),
                    k => format!("{k} up next"),
                };
                let sub = if left > 0 {
                    format!("{sub} · {} left", banshee::model::format_total(left))
                } else {
                    sub
                };
                title.set_subtitle(&sub);
            }
        };
        refresh();
        {
            let refresh = refresh.clone();
            let store = ctl.queue_store.clone();
            let weak_ctl = Rc::downgrade(ctl);
            let list = list.clone();
            ctl.subscribe(move |ev| match ev {
                AppEvent::QueueChanged => refresh(),
                AppEvent::NowPlaying(_) => {
                    // Rebind rows so the now-playing marker and played dimming move.
                    refresh();
                    if let Some(c) = weak_ctl.upgrade() {
                        store.items_changed(0, store.n_items(), store.n_items());
                        // Keep the playing entry in view.
                        if let Some(i) = c.current_index() {
                            list.scroll_to(i as u32, gtk::ListScrollFlags::NONE, None);
                        }
                    }
                }
                _ => {}
            });
        }

        // Window actions for the queue menu.
        let clear = gio::SimpleAction::new("clear-queue", None);
        {
            let ctl = ctl.clone();
            clear.connect_activate(move |_, _| ctl.clear_queue());
        }
        window.add_action(&clear);

        Rc::new(Self { root, header })
    }
}

/// Drag a queue row onto another to move it there.
fn attach_dnd(ctl: &Rc<Controller>, row: &ItemRow) {
    let drag = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    let weak_row = row.downgrade();
    drag.connect_prepare(move |_, _, _| {
        let row = weak_row.upgrade()?;
        match row.item()? {
            RowItem::Queue { index, .. } => {
                Some(gdk::ContentProvider::for_value(&(index as u32).to_value()))
            }
            RowItem::Result(_) => None,
        }
    });
    let weak_row = row.downgrade();
    drag.connect_drag_begin(move |src, _| {
        if let Some(r) = weak_row.upgrade() {
            src.set_icon(Some(&gtk::WidgetPaintable::new(Some(&r))), 0, 0);
        }
    });
    row.add_controller(drag);

    let drop = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
    let (weak_row, ctl) = (row.downgrade(), ctl.clone());
    drop.connect_drop(move |_, value, _, _| {
        let (Some(row), Ok(from)) = (weak_row.upgrade(), value.get::<u32>()) else {
            return false;
        };
        let Some(RowItem::Queue { index: to, .. }) = row.item() else {
            return false;
        };
        ctl.move_entry(from as usize, to);
        true
    });
    row.add_controller(drop);
}
