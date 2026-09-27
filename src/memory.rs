//! Process memory helpers used by the periodic GC and the soak tests (ADR 0010).

/// Resident set size of this process in bytes, from `/proc/self/status` (`VmRSS`).
pub fn rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|l| {
        let rest = l.strip_prefix("VmRSS:")?;
        let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
        Some(kb * 1024)
    })
}

/// Return freed heap pages to the OS (glibc keeps them mapped otherwise).
pub fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim has no preconditions; it only releases free memory.
    unsafe {
        libc::malloc_trim(0);
    }
}
