//! glibc's malloc arenas, capped at [`ARENAS`].
//!
//! glibc gives threads arenas of their own, up to eight per core: 192 on a
//! 24-core machine. Caskade runs well over a hundred threads (tokio's
//! workers, SQLite's, WebKit's), so a burst of work leaves its freed memory
//! spread over many arenas, each holding on to its pages. Measured with an
//! 8,400-game library, the app's resident memory grew past 1 GB within ten
//! minutes by default and stayed near 420 MB with two arenas. Threads keep
//! their own small caches (tcache), so sharing two arenas costs little.
//!
//! Linux with glibc only; other systems' allocators don't keep an arena per
//! thread. A `MALLOC_ARENA_MAX` the user set wins.

#![deny(unsafe_code)]

/// How many arenas malloc may use.
pub const ARENAS: i32 = 2;

/// Caps malloc's arenas at [`ARENAS`], unless the environment already set
/// `MALLOC_ARENA_MAX`. Call first thing in `main`: threads started before
/// it may already have arenas of their own. Returns whether it applied.
pub fn limit_arenas() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        if std::env::var_os("MALLOC_ARENA_MAX").is_some() {
            return false;
        }
        // SAFETY: `mallopt` takes two integers and only changes malloc's
        // settings; glibc makes it safe to call at any time.
        #[allow(unsafe_code)]
        let set = unsafe { libc::mallopt(libc::M_ARENA_MAX, ARENAS) };
        set == 1
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        false
    }
}

/// Hands memory malloc holds free back to the system, every `every`, on a
/// thread of its own. Freed memory between allocations still in use stays
/// resident otherwise: glibc only returns what's free at the top of a heap.
/// Does nothing outside Linux with glibc.
pub fn trim_every(every: std::time::Duration) {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let started = std::thread::Builder::new()
            .name("malloc-trim".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(every);
                    trim();
                }
            });
        if let Err(error) = started {
            eprintln!("malloc trimming not started: {error}");
        }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    let _ = every;
}

/// Hands the free memory malloc holds back to the system now.
pub fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` takes an integer and only releases free pages;
    // glibc makes it safe to call from any thread at any time.
    #[allow(unsafe_code)]
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    fn arenas_are_capped_unless_the_environment_chose() {
        let chosen = std::env::var_os("MALLOC_ARENA_MAX").is_some();
        assert_eq!(super::limit_arenas(), !chosen);
    }
}
