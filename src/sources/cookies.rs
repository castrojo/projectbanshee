//! Flatpak Browser Session Bridge (CONTEXT.md): find Flatpak browser profiles, export their
//! cookies through `yt-dlp --cookies-from-browser`, keep only YouTube/Google cookies, and store
//! the result mode 0600 in `config_dir()/ytm_cookies.txt`. The jar authenticates InnerTube
//! library calls (as a `Cookie:` header) and is handed to `yt-dlp` as a private per-call copy.

use crate::paths;
use crate::sources::{SourceError, SourceResult};
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const JAR_FILE: &str = "ytm_cookies.txt";
const IMPORT_TIMEOUT: Duration = Duration::from_secs(30);
/// Any of these (unexpired) means the jar carries a signed-in Google session.
const AUTH_COOKIES: [&str; 5] = [
    "SID",
    "SAPISID",
    "__Secure-3PSID",
    "__Secure-1PSID",
    "LOGIN_INFO",
];
/// The host InnerTube requests go to; the `Cookie:` header is built for it.
const YTM_HOST: &str = "music.youtube.com";
/// Opening the YouTube Music front page makes yt-dlp load (and save) the browser jar. The
/// generic URL is rejected right after the cookies are written, so no page is fetched.
const IMPORT_PROBE_URL: &str = "https://music.youtube.com";
const JAR_HEADER: &str =
    "# Netscape HTTP Cookie File\n# Filtered by Banshee: YouTube and Google cookies only.\n\n";

/// A Flatpak browser profile yt-dlp can read cookies from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserProfile {
    /// Human-readable name, e.g. "Firefox (Flatpak)".
    pub label: String,
    /// `--cookies-from-browser` argument, e.g. `firefox:/home/me/.var/app/…/abcd.default`.
    pub spec: String,
}

/// Location of the private cookie jar.
pub fn jar_path() -> PathBuf {
    paths::config_dir().join(JAR_FILE)
}

/// Whether an imported jar exists (the YouTube source counts as signed in).
pub fn has_jar() -> bool {
    std::fs::metadata(jar_path())
        .map(|m| m.is_file() && m.len() > 32)
        .unwrap_or(false)
}

/// Contents of the imported jar, if any.
pub(crate) fn read_jar() -> Option<String> {
    match std::fs::read(jar_path()) {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            log::warn!("cannot read the YouTube cookie jar: {e}");
            None
        }
    }
}

/// Remove the imported jar. Signing out when not signed in succeeds.
pub fn sign_out() -> SourceResult<()> {
    let _ = std::fs::remove_file(browser_spec_path());
    match std::fs::remove_file(jar_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SourceError::Unavailable(format!(
            "Could not remove the YouTube cookie jar: {e}"
        ))),
    }
}

/// Flatpak Firefox, Brave and Chrome profiles that have a cookie database. Native (host)
/// browser paths are deliberately not probed: inside the sandbox only `~/.var/app` is shared.
pub fn detect_browsers() -> Vec<BrowserProfile> {
    detect_in(&gtk::glib::home_dir().join(".var/app"))
}

fn detect_in(flatpak_root: &Path) -> Vec<BrowserProfile> {
    let mut found = Vec::new();
    if let Some(profile) =
        firefox_profile(&flatpak_root.join("org.mozilla.firefox/config/mozilla/firefox"))
    {
        found.push(BrowserProfile {
            label: "Firefox (Flatpak)".to_string(),
            spec: format!("firefox:{}", profile.display()),
        });
    }
    let chromium = [
        (
            "Brave (Flatpak)",
            "brave",
            "com.brave.Browser/config/BraveSoftware/Brave-Browser",
        ),
        (
            "Chrome (Flatpak)",
            "chrome",
            "com.google.Chrome/config/google-chrome",
        ),
    ];
    for (label, browser, dir) in chromium {
        if let Some(profile) = chromium_profile(&flatpak_root.join(dir)) {
            found.push(BrowserProfile {
                label: label.to_string(),
                spec: format!("{browser}:{}", profile.display()),
            });
        }
    }
    found
}

/// The first profile (install default, then `Default=1`, then any other) with `cookies.sqlite`.
fn firefox_profile(root: &Path) -> Option<PathBuf> {
    let ini = std::fs::read_to_string(root.join("profiles.ini")).ok()?;
    firefox_profile_candidates(&ini)
        .into_iter()
        .map(|p| {
            if Path::new(&p).is_absolute() {
                PathBuf::from(p)
            } else {
                root.join(p)
            }
        })
        .find(|p| p.join("cookies.sqlite").is_file())
}

/// Profile paths from `profiles.ini`, most-preferred first, without duplicates.
fn firefox_profile_candidates(ini: &str) -> Vec<String> {
    let mut install_defaults = Vec::new();
    let mut marked_default = Vec::new();
    let mut others = Vec::new();
    // (section name, Path, Default, Default=1)
    let mut section = String::new();
    let mut path: Option<String> = None;
    let mut default_key: Option<String> = None;
    let mut is_default = false;
    let mut flush = |section: &str,
                     path: &mut Option<String>,
                     default_key: &mut Option<String>,
                     is_default: &mut bool| {
        if section.starts_with("Install") {
            if let Some(d) = default_key.take() {
                install_defaults.push(d);
            }
        } else if section.starts_with("Profile") {
            if let Some(p) = path.take() {
                if *is_default {
                    marked_default.push(p)
                } else {
                    others.push(p)
                }
            }
        }
        *path = None;
        *default_key = None;
        *is_default = false;
    };
    for line in ini.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            flush(&section, &mut path, &mut default_key, &mut is_default);
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Path" if !value.is_empty() => path = Some(value.to_string()),
            "Default" if section.starts_with("Install") && !value.is_empty() => {
                default_key = Some(value.to_string())
            }
            "Default" => is_default = value == "1",
            _ => {}
        }
    }
    flush(&section, &mut path, &mut default_key, &mut is_default);
    let mut out: Vec<String> = Vec::new();
    for p in install_defaults
        .into_iter()
        .chain(marked_default)
        .chain(others)
    {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// `Default`, then `Profile N` (sorted), whichever first has a cookie database.
fn chromium_profile(root: &Path) -> Option<PathBuf> {
    let mut candidates = vec![root.join("Default")];
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut numbered: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("Profile "))
            .map(|e| e.path())
            .collect();
        numbered.sort();
        candidates.extend(numbered);
    }
    candidates
        .into_iter()
        .find(|p| p.join("Cookies").is_file() || p.join("Network").join("Cookies").is_file())
}

/// Export cookies from a browser profile (a `BrowserProfile::spec`) and store the filtered jar.
/// Must run on the Tokio runtime.
pub async fn import_from_browser(spec: &str) -> SourceResult<()> {
    let dir =
        PrivateTemp::dir().map_err(|e| local_error("prepare a private temporary folder", &e))?;
    let export = dir.path().join("export.txt");
    let mut cmd = tokio::process::Command::new(ytdlp_program());
    cmd.arg("--cookies-from-browser")
        .arg(spec)
        .arg("--cookies")
        .arg(&export)
        .args([
            "--skip-download",
            "--no-warnings",
            "--no-progress",
            "--flat-playlist",
            "--",
            IMPORT_PROBE_URL,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(spawn_error)?;
    let output = match tokio::time::timeout(IMPORT_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(SourceError::Unavailable(format!(
                "yt-dlp failed while reading browser cookies: {e}"
            )));
        }
        Err(_) => {
            return Err(SourceError::Unavailable(format!(
                "Reading browser cookies timed out after {} s",
                IMPORT_TIMEOUT.as_secs()
            )));
        }
    };
    // yt-dlp exits non-zero because the probe URL is not a media page; the exported jar decides.
    let raw = match tokio::fs::read(&export).await {
        Ok(raw) => raw,
        Err(_) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!("cookie import from {spec} failed: {}", stderr.trim());
            return Err(SourceError::Unavailable(format!(
                "Could not read cookies from the browser: {}",
                ytdlp_error_line(&stderr, ErrorLinePick::First)
            )));
        }
    };
    store_filtered(&String::from_utf8_lossy(&raw))?;
    // Remember where the session came from so an expired session can be refreshed.
    if let Err(e) = crate::paths::write_private(&browser_spec_path(), spec.as_bytes()) {
        log::warn!("cannot remember the cookie source browser: {e}");
    }
    Ok(())
}

fn browser_spec_path() -> PathBuf {
    crate::paths::config_dir().join("ytm_browser")
}

/// The browser profile the current session was imported from, if it came from one.
pub fn last_browser_spec() -> Option<String> {
    std::fs::read_to_string(browser_spec_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Import an exported Netscape `cookies.txt`, keeping only YouTube/Google cookies.
pub async fn import_from_file(path: &Path) -> SourceResult<()> {
    let raw = tokio::fs::read(path)
        .await
        .map_err(|e| SourceError::Unavailable(format!("Cannot read {}: {e}", path.display())))?;
    store_filtered(&String::from_utf8_lossy(&raw))?;
    // This session no longer comes from a browser profile.
    let _ = std::fs::remove_file(browser_spec_path());
    Ok(())
}

fn store_filtered(jar_text: &str) -> SourceResult<()> {
    let filtered = filter_jar_at(jar_text, unix_now());
    if !filtered.has_auth {
        return Err(SourceError::AuthRequired(
            "No signed-in YouTube session found; sign in at music.youtube.com in that browser first".to_string(),
        ));
    }
    paths::write_private(&jar_path(), filtered.text.as_bytes())
        .map_err(|e| local_error("save the YouTube cookie jar", &e))?;
    log::info!("imported {} YouTube/Google cookies", filtered.kept);
    Ok(())
}

/// Result of filtering a Netscape jar.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FilteredJar {
    /// Netscape-format jar containing only YouTube/Google cookies.
    text: String,
    kept: usize,
    /// Whether an unexpired sign-in cookie is present.
    has_auth: bool,
}

fn filter_jar_at(jar_text: &str, now: u64) -> FilteredJar {
    let mut text = String::from(JAR_HEADER);
    let mut kept = 0;
    let mut has_auth = false;
    for raw in jar_text.lines() {
        let raw = raw.trim_end_matches('\r');
        let Some(cookie) = Cookie::parse(raw) else {
            continue;
        };
        let domain = cookie.bare_domain();
        let wanted = ["youtube.com", "google.com"]
            .iter()
            .any(|d| domain == *d || domain.ends_with(&format!(".{d}")));
        if !wanted {
            continue;
        }
        text.push_str(raw);
        text.push('\n');
        kept += 1;
        if !cookie.expired(now)
            && AUTH_COOKIES
                .iter()
                .any(|n| n.eq_ignore_ascii_case(cookie.name))
        {
            has_auth = true;
        }
    }
    FilteredJar {
        text,
        kept,
        has_auth,
    }
}

/// The `Cookie:` header value for music.youtube.com: unexpired cookies whose domain covers
/// the host. `SAPISID` comes first (InnerTube derives its auth hash from it).
pub fn cookie_header(jar_text: &str) -> Option<String> {
    cookie_header_at(jar_text, unix_now())
}

fn cookie_header_at(jar_text: &str, now: u64) -> Option<String> {
    // (name, value, specificity of the matching domain)
    let mut chosen: Vec<(&str, &str, usize)> = Vec::new();
    for raw in jar_text.lines() {
        let Some(cookie) = Cookie::parse(raw.trim_end_matches('\r')) else {
            continue;
        };
        if cookie.expired(now) || !cookie.applies_to(YTM_HOST) || !cookie.header_safe() {
            continue;
        }
        let specificity = cookie.bare_domain().len();
        match chosen.iter_mut().find(|(name, _, _)| *name == cookie.name) {
            Some(slot) if specificity >= slot.2 => *slot = (cookie.name, cookie.value, specificity),
            Some(_) => {}
            None => chosen.push((cookie.name, cookie.value, specificity)),
        }
    }
    if chosen.is_empty() {
        return None;
    }
    if let Some(pos) = chosen.iter().position(|(name, _, _)| *name == "SAPISID") {
        let sapisid = chosen.remove(pos);
        chosen.insert(0, sapisid);
    }
    let header = chosen
        .iter()
        .map(|(n, v, _)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    Some(header)
}

/// One Netscape cookie line: domain, include-subdomains, path, secure, expiry, name, value.
struct Cookie<'a> {
    domain: &'a str,
    include_subdomains: bool,
    path: &'a str,
    expires: i64,
    name: &'a str,
    value: &'a str,
}

impl<'a> Cookie<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            return None;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [
            domain,
            include_subdomains,
            path,
            _secure,
            expires,
            name,
            value,
        ] = fields.as_slice()
        else {
            return None;
        };
        let domain = domain.strip_prefix("#HttpOnly_").unwrap_or(domain);
        if domain.is_empty() || name.is_empty() {
            return None;
        }
        Some(Cookie {
            domain,
            include_subdomains: include_subdomains.eq_ignore_ascii_case("TRUE"),
            path,
            expires: expires.trim().parse().ok()?,
            name,
            value,
        })
    }

    fn bare_domain(&self) -> String {
        self.domain.trim_start_matches('.').to_ascii_lowercase()
    }

    /// Session cookies (expiry 0) never expire while the jar exists.
    fn expired(&self, now: u64) -> bool {
        self.expires > 0
            && u64::try_from(self.expires)
                .map(|e| e <= now)
                .unwrap_or(true)
    }

    fn applies_to(&self, host: &str) -> bool {
        let bare = self.bare_domain();
        let domain_ok = bare == host
            || ((self.include_subdomains || self.domain.starts_with('.'))
                && host.ends_with(&format!(".{bare}")));
        domain_ok && (self.path.is_empty() || self.path == "/")
    }

    fn header_safe(&self) -> bool {
        let bad = |s: &str| s.chars().any(|c| matches!(c, ';' | '\r' | '\n' | '\0'));
        !bad(self.name) && !bad(self.value) && !self.name.contains('=')
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `yt-dlp` executable: `BANSHEE_YTDLP` if set, else `yt-dlp` on `PATH`.
pub(crate) fn ytdlp_program() -> OsString {
    std::env::var_os("BANSHEE_YTDLP")
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| OsString::from("yt-dlp"))
}

/// Map a failure to start yt-dlp.
pub(crate) fn spawn_error(e: std::io::Error) -> SourceError {
    if e.kind() == std::io::ErrorKind::NotFound {
        SourceError::Extraction("yt-dlp not found".to_string())
    } else {
        SourceError::Extraction(format!("cannot start yt-dlp: {e}"))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ErrorLinePick {
    First,
    Last,
}

/// The most useful line of yt-dlp's stderr: an `ERROR:` line (first or last) without the
/// `ERROR:` / `[extractor] id:` prefixes, else the last non-empty line.
pub(crate) fn ytdlp_error_line(stderr: &str, pick: ErrorLinePick) -> String {
    let lines = stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    let mut errors = lines.clone().filter(|l| l.starts_with("ERROR:"));
    let line = match pick {
        ErrorLinePick::First => errors.next(),
        ErrorLinePick::Last => errors.next_back(),
    }
    .or_else(|| lines.clone().next_back())
    .unwrap_or("yt-dlp failed without an error message");
    let line = line.strip_prefix("ERROR:").unwrap_or(line).trim_start();
    let line = match line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
    {
        Some((_, rest)) => match rest.split_once(": ") {
            Some((id, msg)) if !id.contains(' ') => msg,
            _ => rest,
        },
        None => line,
    };
    line.to_string()
}

fn local_error(action: &str, e: &std::io::Error) -> SourceError {
    SourceError::Unavailable(format!("Could not {action}: {e}"))
}

/// A private (0600 file / 0700 directory) temporary path under the user runtime directory,
/// removed on drop. Used for per-call yt-dlp jars so yt-dlp never rewrites the stored jar.
pub(crate) struct PrivateTemp {
    path: PathBuf,
    is_dir: bool,
}

impl PrivateTemp {
    fn base() -> std::io::Result<PathBuf> {
        let base = gtk::glib::user_runtime_dir().join("banshee");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&base)?;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))?;
        Ok(base)
    }

    fn unique_name(prefix: &str) -> String {
        format!(
            "{prefix}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        )
    }

    /// A fresh private directory.
    pub(crate) fn dir() -> std::io::Result<Self> {
        let path = Self::base()?.join(Self::unique_name("cookies"));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(PrivateTemp { path, is_dir: true })
    }

    /// A fresh private file holding `contents`.
    pub(crate) fn file(contents: &[u8]) -> std::io::Result<Self> {
        let path = Self::base()?.join(Self::unique_name("jar"));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let temp = PrivateTemp {
            path,
            is_dir: false,
        };
        f.write_all(contents)?;
        Ok(temp)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateTemp {
    fn drop(&mut self) {
        let result = if self.is_dir {
            std::fs::remove_dir_all(&self.path)
        } else {
            std::fs::remove_file(&self.path)
        };
        if let Err(e) = result
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!(
                "cannot remove temporary cookie file {}: {e}",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;
    const FUTURE: &str = "1900000000";
    const PAST: &str = "1700000000";

    fn line(domain: &str, sub: &str, name: &str, value: &str, expires: &str) -> String {
        format!("{domain}\t{sub}\t/\tTRUE\t{expires}\t{name}\t{value}")
    }

    #[test]
    fn filter_keeps_only_youtube_and_google_cookies() {
        let jar = [
            "# Netscape HTTP Cookie File".to_string(),
            line(".youtube.com", "TRUE", "PREF", "f6=40000000", FUTURE),
            line("#HttpOnly_.google.com", "TRUE", "NID", "abc", FUTURE),
            line("accounts.google.com", "FALSE", "LSID", "x", FUTURE),
            line(".notyoutube.com", "TRUE", "PREF", "evil", FUTURE),
            line(".google.com.evil.net", "TRUE", "SID", "evil", FUTURE),
            line(".dell.com", "TRUE", "SID", "other", FUTURE),
            "malformed\tline".to_string(),
        ]
        .join("\n");
        let filtered = filter_jar_at(&jar, NOW);
        assert_eq!(filtered.kept, 3);
        assert!(filtered.text.starts_with("# Netscape HTTP Cookie File\n"));
        assert!(filtered.text.contains("f6=40000000"));
        assert!(
            filtered
                .text
                .contains("#HttpOnly_.google.com\tTRUE\t/\tTRUE\t1900000000\tNID\tabc")
        );
        assert!(filtered.text.contains("LSID"));
        assert!(!filtered.text.contains("evil"));
        assert!(!filtered.text.contains("other"));
        assert!(!filtered.has_auth, "no sign-in cookie among the kept ones");
    }

    #[test]
    fn filter_requires_an_unexpired_sign_in_cookie() {
        let expired = line(".youtube.com", "TRUE", "SAPISID", "old", PAST);
        assert!(!filter_jar_at(&expired, NOW).has_auth);
        let foreign = line(".example.com", "TRUE", "SAPISID", "x", FUTURE);
        assert!(!filter_jar_at(&foreign, NOW).has_auth);
        for name in AUTH_COOKIES {
            let jar = line("#HttpOnly_.youtube.com", "TRUE", name, "v", FUTURE);
            assert!(
                filter_jar_at(&jar, NOW).has_auth,
                "{name} counts as sign-in"
            );
        }
        let session = line(".google.com", "TRUE", "SID", "v", "0");
        assert!(
            filter_jar_at(&session, NOW).has_auth,
            "session cookies are unexpired"
        );
    }

    #[test]
    fn header_includes_only_cookies_sent_to_music_youtube_com() {
        let jar = [
            line(".youtube.com", "TRUE", "PREF", "p", FUTURE),
            line("music.youtube.com", "FALSE", "YSC", "y", "0"),
            line("www.youtube.com", "FALSE", "WWW", "no", FUTURE),
            line("youtube.com", "FALSE", "HOSTONLY", "no", FUTURE),
            line(".google.com", "TRUE", "NID", "no", FUTURE),
            line(".youtube.com", "TRUE", "OLD", "no", PAST),
            line(".youtube.com", "TRUE", "BAD", "a;b", FUTURE),
            "#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1900000000\tSAPISID\ts".to_string(),
            ".youtube.com\tTRUE\t/watch\tTRUE\t1900000000\tPATHY\tno".to_string(),
        ]
        .join("\n");
        assert_eq!(
            cookie_header_at(&jar, NOW).as_deref(),
            Some("SAPISID=s; PREF=p; YSC=y")
        );
    }

    #[test]
    fn header_prefers_the_most_specific_domain_for_duplicate_names() {
        let jar = [
            line(".youtube.com", "TRUE", "PREF", "general", FUTURE),
            line(".music.youtube.com", "TRUE", "PREF", "specific", FUTURE),
            line(".youtube.com", "TRUE", "PREF", "general-again", FUTURE),
        ]
        .join("\n");
        assert_eq!(
            cookie_header_at(&jar, NOW).as_deref(),
            Some("PREF=specific")
        );
    }

    #[test]
    fn header_is_none_without_applicable_cookies() {
        assert_eq!(cookie_header_at("", NOW), None);
        assert_eq!(
            cookie_header_at(&line(".google.com", "TRUE", "SID", "x", FUTURE), NOW),
            None
        );
    }

    #[test]
    fn firefox_candidates_prefer_install_default_then_marked_default() {
        let ini = "[General]\nStartWithLastProfile=1\n\n[Profile0]\nName=default-release\nIsRelative=1\nPath=a.default-release\n\n[InstallCF14]\nDefault=a.default-release\nLocked=1\n\n[Profile1]\nName=default\nIsRelative=1\nPath=b.default\nDefault=1\n\n[Profile2]\nPath=/abs/c\n";
        assert_eq!(
            firefox_profile_candidates(ini),
            vec!["a.default-release", "b.default", "/abs/c"]
        );
    }

    #[test]
    fn detects_flatpak_profiles_with_cookie_databases() {
        let root = tempfile::tempdir().expect("tempdir");
        let ff = root
            .path()
            .join("org.mozilla.firefox/config/mozilla/firefox");
        std::fs::create_dir_all(ff.join("empty.default")).expect("mkdir");
        std::fs::create_dir_all(ff.join("real.default-release")).expect("mkdir");
        std::fs::write(ff.join("real.default-release/cookies.sqlite"), b"").expect("write");
        std::fs::write(
            ff.join("profiles.ini"),
            "[Profile0]\nPath=empty.default\nDefault=1\n[Profile1]\nPath=real.default-release\n",
        )
        .expect("write");
        let brave = root
            .path()
            .join("com.brave.Browser/config/BraveSoftware/Brave-Browser");
        std::fs::create_dir_all(brave.join("Default")).expect("mkdir");
        std::fs::create_dir_all(brave.join("Profile 2/Network")).expect("mkdir");
        std::fs::write(brave.join("Profile 2/Network/Cookies"), b"").expect("write");

        let found = detect_in(root.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].label, "Firefox (Flatpak)");
        assert_eq!(
            found[0].spec,
            format!("firefox:{}", ff.join("real.default-release").display())
        );
        assert_eq!(found[1].label, "Brave (Flatpak)");
        assert_eq!(
            found[1].spec,
            format!("brave:{}", brave.join("Profile 2").display())
        );
    }

    #[test]
    fn error_line_strips_prefixes() {
        let stderr = "[youtube] abc: Downloading webpage\nERROR: [youtube] abc: Video unavailable\nERROR: second\n";
        assert_eq!(
            ytdlp_error_line(stderr, ErrorLinePick::First),
            "Video unavailable"
        );
        assert_eq!(ytdlp_error_line(stderr, ErrorLinePick::Last), "second");
        assert_eq!(
            ytdlp_error_line("just text\n", ErrorLinePick::Last),
            "just text"
        );
    }
}
