//! The process's CPU time, to keep a backdrop's frames within their share
//! of a core however much presenting a frame costs (a virtual display that
//! copies every frame on the CPU, a remote X connection).

use std::time::Duration;

/// The CPU time every thread of the process has used, where the system
/// says.
pub(crate) fn process() -> Option<Duration> {
    #[cfg(unix)]
    {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `clock_gettime` writes the given timespec, which lives
        // for the call.
        let read = unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut time) };
        (read == 0).then(|| Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: the pseudo-handle of the current process, and four
        // FILETIMEs that live for the call.
        let read = unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) };
        let ticks = |time: &FILETIME| {
            (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
        };
        (read != 0).then(|| Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}
