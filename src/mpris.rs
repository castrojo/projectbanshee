//! MPRIS v2 (`org.mpris.MediaPlayer2` + `.Player`) over GDBus.
//!
//! The service is a thin adapter: it reads a snapshot from an [`MprisTarget`] and forwards
//! commands to it. The app controller calls [`Mpris::notify`] when state changes.

use crate::model::Track;
use crate::queue::RepeatMode;
use gtk::gio;
use gtk::glib::{self, Variant, VariantDict, prelude::*};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const BUS_NAME: &str = "org.mpris.MediaPlayer2.banshee";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

const XML: &str = r#"
<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/>
    <method name="Previous"/>
    <method name="Pause"/>
    <method name="PlayPause"/>
    <method name="Stop"/>
    <method name="Play"/>
    <method name="Seek"><arg direction="in" type="x" name="Offset"/></method>
    <method name="SetPosition">
      <arg direction="in" type="o" name="TrackId"/>
      <arg direction="in" type="x" name="Position"/>
    </method>
    <method name="OpenUri"><arg direction="in" type="s" name="Uri"/></method>
    <signal name="Seeked"><arg name="Position" type="x"/></signal>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="LoopStatus" type="s" access="readwrite"/>
    <property name="Rate" type="d" access="readwrite"/>
    <property name="Shuffle" type="b" access="readwrite"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Volume" type="d" access="readwrite"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
</node>"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Playing,
    Paused,
    Stopped,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Playing => "Playing",
            Status::Paused => "Paused",
            Status::Stopped => "Stopped",
        }
    }
}

/// Snapshot the service exposes.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub status: Status,
    /// Current queue entry id and its Track.
    pub current: Option<(u64, Track)>,
    pub length_us: Option<i64>,
    pub position_us: i64,
    pub volume: f64,
    pub can_next: bool,
    pub can_previous: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// Local file URI of the artwork if cached, else the remote URL.
    pub art_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Raise,
    Quit,
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Relative seek in microseconds.
    Seek(i64),
    /// Absolute seek for the given entry, microseconds.
    SetPosition {
        entry_id: u64,
        position_us: i64,
    },
    OpenUri(String),
    SetVolume(f64),
    SetShuffle(bool),
    SetRepeat(RepeatMode),
}

pub trait MprisTarget {
    fn snapshot(&self) -> Snapshot;
    fn command(&self, cmd: Command);
}

pub fn track_object_path(entry_id: u64) -> String {
    format!("/io/github/castrojo/Banshee/entry/{entry_id}")
}

fn parse_entry_path(path: &str) -> Option<u64> {
    path.strip_prefix("/io/github/castrojo/Banshee/entry/")?
        .parse()
        .ok()
}

fn loop_str(r: RepeatMode) -> &'static str {
    match r {
        RepeatMode::Off => "None",
        RepeatMode::One => "Track",
        RepeatMode::All => "Playlist",
    }
}

fn metadata(s: &Snapshot) -> Variant {
    let d = VariantDict::new(None);
    match &s.current {
        Some((entry, t)) => {
            let path = glib::variant::ObjectPath::try_from(track_object_path(*entry))
                .unwrap_or_else(|_| {
                    glib::variant::ObjectPath::try_from(
                        "/org/mpris/MediaPlayer2/TrackList/NoTrack".to_string(),
                    )
                    .expect("static path is valid")
                });
            d.insert_value("mpris:trackid", &path.to_variant());
            d.insert_value("xesam:title", &t.title.to_variant());
            d.insert_value("xesam:artist", &vec![t.artist.clone()].to_variant());
            if let Some(a) = &t.album {
                d.insert_value("xesam:album", &a.to_variant());
            }
            d.insert_value("xesam:url", &t.web_url().to_variant());
            if let Some(l) = s.length_us {
                d.insert_value("mpris:length", &l.to_variant());
            }
            if let Some(art) = &s.art_url {
                d.insert_value("mpris:artUrl", &art.to_variant());
            }
        }
        None => {
            let path = glib::variant::ObjectPath::try_from(
                "/org/mpris/MediaPlayer2/TrackList/NoTrack".to_string(),
            )
            .expect("static path is valid");
            d.insert_value("mpris:trackid", &path.to_variant());
        }
    }
    d.end()
}

fn player_props(s: &Snapshot) -> HashMap<&'static str, Variant> {
    let has = s.current.is_some();
    HashMap::from([
        ("PlaybackStatus", s.status.as_str().to_variant()),
        ("LoopStatus", loop_str(s.repeat).to_variant()),
        ("Rate", 1.0f64.to_variant()),
        ("Shuffle", s.shuffle.to_variant()),
        ("Metadata", metadata(s)),
        ("Volume", s.volume.to_variant()),
        ("Position", s.position_us.to_variant()),
        ("MinimumRate", 1.0f64.to_variant()),
        ("MaximumRate", 1.0f64.to_variant()),
        ("CanGoNext", s.can_next.to_variant()),
        ("CanGoPrevious", s.can_previous.to_variant()),
        ("CanPlay", has.to_variant()),
        ("CanPause", has.to_variant()),
        ("CanSeek", (has && s.length_us.is_some()).to_variant()),
        ("CanControl", true.to_variant()),
    ])
}

fn root_prop(name: &str) -> Option<Variant> {
    Some(match name {
        "CanQuit" | "CanRaise" => true.to_variant(),
        "HasTrackList" => false.to_variant(),
        "Identity" => "Banshee".to_variant(),
        "DesktopEntry" => crate::paths::APP_ID.to_variant(),
        "SupportedUriSchemes" => vec!["https", "spotify"].to_variant(),
        "SupportedMimeTypes" => Vec::<String>::new().to_variant(),
        _ => return None,
    })
}

struct Inner {
    conn: Option<gio::DBusConnection>,
    last: HashMap<&'static str, Variant>,
    registrations: Vec<gio::RegistrationId>,
}

/// Owns the bus name for the lifetime of the value.
pub struct Mpris {
    inner: Rc<RefCell<Inner>>,
    target: Rc<dyn MprisTarget>,
    owner: Option<gio::OwnerId>,
}

impl Mpris {
    pub fn new(target: Rc<dyn MprisTarget>) -> Self {
        let inner = Rc::new(RefCell::new(Inner {
            conn: None,
            last: HashMap::new(),
            registrations: vec![],
        }));
        let node = match gio::DBusNodeInfo::for_xml(XML) {
            Ok(n) => n,
            Err(e) => {
                log::error!("MPRIS introspection XML invalid: {e}");
                return Self {
                    inner,
                    target,
                    owner: None,
                };
            }
        };
        let (i2, t2) = (inner.clone(), target.clone());
        let owner = gio::bus_own_name(
            gio::BusType::Session,
            BUS_NAME,
            gio::BusNameOwnerFlags::DO_NOT_QUEUE,
            move |conn, _| {
                for iface in [ROOT_IFACE, PLAYER_IFACE] {
                    let Some(info) = node.lookup_interface(iface) else {
                        continue;
                    };
                    let (tc, tp, ts) = (t2.clone(), t2.clone(), t2.clone());
                    let res = conn
                        .register_object(PATH, &info)
                        .method_call(move |_, _, _, iface, method, params, inv| {
                            handle_call(&*tc, iface.unwrap_or(""), method, &params, inv)
                        })
                        .property(move |_, _, _, iface, prop| {
                            if iface == ROOT_IFACE {
                                return root_prop(prop).unwrap_or_else(|| false.to_variant());
                            }
                            let s = tp.snapshot();
                            player_props(&s)
                                .remove(prop)
                                .unwrap_or_else(|| false.to_variant())
                        })
                        .set_property(move |_, _, _, _, prop, value| set_prop(&*ts, prop, &value))
                        .build();
                    match res {
                        Ok(id) => i2.borrow_mut().registrations.push(id),
                        Err(e) => log::error!("MPRIS register {iface}: {e}"),
                    }
                }
                i2.borrow_mut().conn = Some(conn);
            },
            |_, _| log::info!("MPRIS name {BUS_NAME} acquired"),
            |_, _| log::warn!("MPRIS name {BUS_NAME} lost or unavailable (another instance?)"),
        );
        Self {
            inner,
            target,
            owner: Some(owner),
        }
    }

    /// Emit PropertiesChanged for whatever differs from the last notification.
    /// Position is excluded per spec (clients interpolate; `Seeked` announces jumps).
    pub fn notify(&self) {
        let s = self.target.snapshot();
        let mut props = player_props(&s);
        props.remove("Position");
        let mut inner = self.inner.borrow_mut();
        let Some(conn) = inner.conn.clone() else {
            return;
        };
        let changed: Vec<(&'static str, Variant)> = props
            .into_iter()
            .filter(|(k, v)| inner.last.get(k) != Some(v))
            .collect();
        if changed.is_empty() {
            return;
        }
        let dict = VariantDict::new(None);
        for (k, v) in &changed {
            dict.insert_value(k, v);
            inner.last.insert(k, v.clone());
        }
        let params = (PLAYER_IFACE, dict.end(), Vec::<String>::new()).to_variant();
        if let Err(e) = conn.emit_signal(
            None,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(&params),
        ) {
            log::warn!("MPRIS PropertiesChanged: {e}");
        }
    }

    pub fn seeked(&self, position_us: i64) {
        let conn = self.inner.borrow().conn.clone();
        if let Some(conn) = conn {
            if let Err(e) = conn.emit_signal(
                None,
                PATH,
                PLAYER_IFACE,
                "Seeked",
                Some(&(position_us,).to_variant()),
            ) {
                log::warn!("MPRIS Seeked: {e}");
            }
        }
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        let mut inner = self.inner.borrow_mut();
        if let Some(conn) = inner.conn.take() {
            for id in inner.registrations.drain(..) {
                let _ = conn.unregister_object(id);
            }
        }
        if let Some(o) = self.owner.take() {
            gio::bus_unown_name(o);
        }
    }
}

fn set_prop(t: &dyn MprisTarget, prop: &str, value: &Variant) -> bool {
    match prop {
        "Volume" => value
            .get::<f64>()
            .map(|v| t.command(Command::SetVolume(v.clamp(0.0, 1.0))))
            .is_some(),
        "Shuffle" => value
            .get::<bool>()
            .map(|v| t.command(Command::SetShuffle(v)))
            .is_some(),
        "LoopStatus" => match value.get::<String>().as_deref() {
            Some("None") => {
                t.command(Command::SetRepeat(RepeatMode::Off));
                true
            }
            Some("Track") => {
                t.command(Command::SetRepeat(RepeatMode::One));
                true
            }
            Some("Playlist") => {
                t.command(Command::SetRepeat(RepeatMode::All));
                true
            }
            _ => false,
        },
        "Rate" => true, // Only 1.0 is supported; ignore per spec.
        _ => false,
    }
}

fn handle_call(
    t: &dyn MprisTarget,
    iface: &str,
    method: &str,
    params: &Variant,
    inv: gio::DBusMethodInvocation,
) {
    let cmd =
        match (iface, method) {
            (ROOT_IFACE, "Raise") => Some(Command::Raise),
            (ROOT_IFACE, "Quit") => Some(Command::Quit),
            (PLAYER_IFACE, "Play") => Some(Command::Play),
            (PLAYER_IFACE, "Pause") => Some(Command::Pause),
            (PLAYER_IFACE, "PlayPause") => Some(Command::PlayPause),
            (PLAYER_IFACE, "Stop") => Some(Command::Stop),
            (PLAYER_IFACE, "Next") => Some(Command::Next),
            (PLAYER_IFACE, "Previous") => Some(Command::Previous),
            (PLAYER_IFACE, "Seek") => params.get::<(i64,)>().map(|(o,)| Command::Seek(o)),
            (PLAYER_IFACE, "SetPosition") => params
                .get::<(glib::variant::ObjectPath, i64)>()
                .and_then(|(p, pos)| {
                    parse_entry_path(p.as_str()).map(|entry_id| Command::SetPosition {
                        entry_id,
                        position_us: pos,
                    })
                }),
            (PLAYER_IFACE, "OpenUri") => params.get::<(String,)>().map(|(u,)| Command::OpenUri(u)),
            _ => {
                inv.return_error(
                    gio::DBusError::UnknownMethod,
                    &format!("Unknown method {iface}.{method}"),
                );
                return;
            }
        };
    match cmd {
        Some(c) => {
            // Reply first so clients never block on the command's side effects.
            inv.return_value(None);
            t.command(c);
        }
        None if method == "SetPosition" => inv.return_value(None), // stale track id: ignore per spec
        None => inv.return_error(gio::DBusError::InvalidArgs, "Invalid arguments"),
    }
}
