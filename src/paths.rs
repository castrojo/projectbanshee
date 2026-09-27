//! App-scoped directories. Inside Flatpak these resolve to `~/.var/app/<app-id>/…`.

use std::path::PathBuf;

pub const APP_ID: &str = "io.github.castrojo.Banshee";

fn ensure(p: PathBuf) -> PathBuf {
    if let Err(e) = std::fs::create_dir_all(&p) {
        log::error!("cannot create {}: {e}", p.display());
    }
    p
}

/// Private configuration (credentials live here, mode 0600).
pub fn config_dir() -> PathBuf {
    ensure(gtk::glib::user_config_dir().join("banshee"))
}

/// Disposable caches (library JSON, artwork).
pub fn cache_dir() -> PathBuf {
    ensure(gtk::glib::user_cache_dir().join("banshee"))
}

/// Persistent app state (queue, preferences).
pub fn state_dir() -> PathBuf {
    ensure(gtk::glib::user_data_dir().join("banshee"))
}

/// Write `data` atomically with mode 0600.
pub fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(tmp, path)
}
