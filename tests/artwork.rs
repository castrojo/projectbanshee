use banshee::artwork::{ArtworkStore, DiskCache, sized_thumbnail};
use banshee::memory;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::time::{Duration, SystemTime};

fn texture(px: i32, seed: u8) -> gdk::Texture {
    let bytes = vec![seed; (px * px * 4) as usize];
    gdk::MemoryTexture::new(
        px,
        px,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(bytes),
        (px * 4) as usize,
    )
    .into()
}

#[test]
fn thumbnail_urls_are_resized_for_google_image_servers_only() {
    assert_eq!(
        sized_thumbnail("https://lh3.googleusercontent.com/abc=w60-h60-l90-rj", 226),
        "https://lh3.googleusercontent.com/abc=w226-h226-l90-rj"
    );
    let yt = "https://i.ytimg.com/vi/xyz/hq720.jpg?sqp=abc&rs=def";
    assert_eq!(
        sized_thumbnail(yt, 120),
        "https://i.ytimg.com/vi/xyz/mqdefault.jpg"
    );
    assert_eq!(
        sized_thumbnail(yt, 320),
        "https://i.ytimg.com/vi/xyz/hqdefault.jpg"
    );
    let other = "https://example.org/cover.png";
    assert_eq!(sized_thumbnail(other, 120), other);
}

#[test]
fn disk_cache_prunes_least_recently_accessed_files_to_budget() {
    let dir = tempfile::tempdir().unwrap();
    let d = DiskCache::new(dir.path(), 2500).unwrap();
    d.put("old", &[0; 1000]).unwrap();
    d.put("mid", &[0; 1000]).unwrap();
    d.put("new", &[0; 1000]).unwrap();
    // Age files explicitly, then read "old" so it becomes most recently used.
    let base = SystemTime::now() - Duration::from_secs(1000);
    for (i, e) in std::fs::read_dir(dir.path()).unwrap().flatten().enumerate() {
        std::fs::File::options()
            .append(true)
            .open(e.path())
            .unwrap()
            .set_modified(base + Duration::from_secs(i as u64))
            .unwrap();
    }
    assert!(d.get("old").is_some());
    let (removed, left) = d.prune().unwrap();
    assert_eq!(removed, 1);
    assert!(left <= 2500);
    assert!(d.get("old").is_some());
    assert_eq!(
        [d.get("mid").is_some(), d.get("new").is_some()]
            .iter()
            .filter(|x| **x)
            .count(),
        1
    );
}

#[test]
fn memory_store_stays_within_byte_budget_and_rss_stays_flat() {
    let budget = 8 * 1024 * 1024;
    let dir = tempfile::tempdir().unwrap();
    let store = ArtworkStore::new(
        DiskCache::new(dir.path(), 1 << 20).unwrap(),
        budget,
        reqwest::Client::new(),
    );
    // Warm up to the budget, then measure while churning through 10x as many textures.
    for i in 0..200 {
        store.insert(&format!("warm{i}"), texture(128, i as u8));
    }
    store.collect();
    memory::trim_heap();
    let baseline = memory::rss_bytes().unwrap();
    let mut peak = baseline;
    for i in 0..5_000u32 {
        store.insert(&format!("u{i}"), texture(128, (i % 251) as u8));
        assert!(store.memory_bytes() <= budget);
        if i % 500 == 0 {
            store.collect();
            memory::trim_heap();
            peak = peak.max(memory::rss_bytes().unwrap());
        }
    }
    // 5000 × 64 KiB = 312 MiB was decoded; resident growth must stay far below that.
    let growth = peak.saturating_sub(baseline);
    assert!(
        growth < 24 * 1024 * 1024,
        "RSS grew by {} MiB",
        growth / (1024 * 1024)
    );
    assert_eq!(store.memory_len(), budget / (128 * 128 * 4));
    store.clear_memory();
    assert_eq!(store.memory_bytes(), 0);
}

fn png(px: i32) -> Vec<u8> {
    texture(px, 200).save_to_png_bytes().to_vec()
}

fn block_on_with_timeout<T>(fut: impl std::future::Future<Output = T>) -> Option<T> {
    let ctx = glib::MainContext::default();
    ctx.block_on(async move {
        futures::select! {
            v = futures::FutureExt::fuse(Box::pin(fut)) => Some(v),
            _ = futures::FutureExt::fuse(glib::timeout_future(Duration::from_secs(10))) => None,
        }
    })
}

#[test]
fn a_cancelled_first_load_does_not_wedge_later_loads_of_the_same_url() {
    let dir = tempfile::tempdir().unwrap();
    let disk = DiskCache::new(dir.path(), 1 << 24).unwrap();
    disk.put("test://cover", &png(64)).unwrap();
    let store = ArtworkStore::new(disk, 1 << 24, reqwest::Client::new());
    // First caller starts the fetch, then goes away (e.g. its row was recycled).
    let first = store.load("test://cover");
    assert!(futures::FutureExt::now_or_never(Box::pin(first)).is_none());
    let tex = block_on_with_timeout(store.load("test://cover")).expect("second load hung");
    assert_eq!(tex.unwrap().width(), 64);
}

#[test]
fn large_artwork_is_downscaled_when_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let disk = DiskCache::new(dir.path(), 1 << 26).unwrap();
    disk.put("test://huge", &png(1200)).unwrap();
    let store = ArtworkStore::new(disk, 1 << 26, reqwest::Client::new());
    let tex = block_on_with_timeout(store.load("test://huge"))
        .expect("load hung")
        .unwrap();
    assert!(
        tex.width() <= banshee::artwork::MAX_DECODE_PX
            && tex.height() <= banshee::artwork::MAX_DECODE_PX
    );
    assert!(
        store.memory_bytes()
            <= (banshee::artwork::MAX_DECODE_PX * banshee::artwork::MAX_DECODE_PX * 4) as usize
    );
}

fn solid(px: i32, rgba: [u8; 4]) -> gdk::Texture {
    let bytes: Vec<u8> = std::iter::repeat_n(rgba, (px * px) as usize)
        .flatten()
        .collect();
    gdk::MemoryTexture::new(
        px,
        px,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(bytes),
        (px * 4) as usize,
    )
    .into()
}

fn contrast_with_white((r, g, b): (u8, u8, u8)) -> f64 {
    let c = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b);
    1.05 / (l + 0.05)
}

#[test]
fn accent_follows_the_artworks_hue_and_keeps_white_text_readable() {
    let (r, g, b) = banshee::artwork::dominant_color(&solid(64, [230, 40, 40, 255]));
    assert!(r > g * 2 && r > b * 2, "expected red, got {r},{g},{b}");
    // A pale yellow cover must be darkened enough for white text on the play button.
    let yellow = banshee::artwork::dominant_color(&solid(64, [250, 240, 120, 255]));
    assert!(contrast_with_white(yellow) >= 4.5, "{yellow:?}");
    assert!(
        yellow.0 > yellow.2 && yellow.1 > yellow.2,
        "still yellowish: {yellow:?}"
    );
}
