//! The resident memory of the process, which the worker reports with every document so that its
//! caller can replace a worker that has grown (docs/protocol.md).

/// The resident memory of the process, in bytes, as far as the system tells it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Resident {
    /// The resident size now (Linux).
    pub(crate) now: Option<u64>,
    /// The largest resident size the process has had (Linux and macOS).
    pub(crate) peak: Option<u64>,
}

/// The resident memory of this process: on Linux both sizes, from `/proc/self/status` (in
/// kibibytes, so the page size that `/proc/self/statm` counts in is not needed); on macOS the
/// peak, from `getrusage`, as the size now needs a call into the kernel that only unsafe code can
/// make; elsewhere nothing.
pub(crate) fn resident() -> Resident {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/status")
            .map_or_else(|_| Resident::default(), |status| parse_status(&status))
    }
    #[cfg(target_os = "macos")]
    {
        use nix::sys::resource::{UsageWho, getrusage};
        // In bytes on macOS (in kibibytes on Linux and the BSDs).
        Resident {
            now: None,
            peak: getrusage(UsageWho::RUSAGE_SELF)
                .ok()
                .and_then(|usage| u64::try_from(usage.max_rss()).ok()),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Resident::default()
    }
}

/// The sizes in the text of `/proc/self/status`: `VmRSS` and `VmHWM`, in kibibytes.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn parse_status(status: &str) -> Resident {
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.trim().strip_suffix("kB"))
            .and_then(|kibibytes| kibibytes.trim().parse::<u64>().ok())
            .map(|kibibytes| kibibytes * 1024)
    };
    Resident {
        now: field("VmRSS:"),
        peak: field("VmHWM:"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_proc_self_status() {
        let status =
            "Name:\tfagus\nVmPeak:\t  420000 kB\nVmHWM:\t   81234 kB\nVmRSS:\t   40960 kB\nThreads:\t3\n";
        assert_eq!(
            parse_status(status),
            Resident {
                now: Some(40960 * 1024),
                peak: Some(81234 * 1024)
            }
        );
        assert_eq!(parse_status("Name:\tfagus\n"), Resident::default());
    }

    #[test]
    fn knows_its_own_memory_where_the_system_tells_it() {
        let resident = resident();
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            // At least the binary's pages are resident.
            assert!(resident.peak.is_some_and(|peak| peak > 1 << 20), "{resident:?}");
        }
        if cfg!(target_os = "linux") {
            assert!(
                resident
                    .now
                    .is_some_and(|now| now > 1 << 20 && Some(now) <= resident.peak)
            );
        }
    }
}
