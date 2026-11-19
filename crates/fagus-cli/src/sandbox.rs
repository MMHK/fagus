//! The sandbox that `fagus render` and `fagus worker` render in on Linux (docs/cli.md#sandbox).
//!
//! Rendering needs no network, starts no programs and writes no files: it reads the document's
//! files and fonts and writes the PDF to a file opened before, or to standard output. Once the
//! inputs are read and the output is open, the process gives up everything else, as far as the
//! kernel allows:
//!
//! - Landlock (Linux 5.13 and later) limits the files it may read to the given directories, and
//!   forbids writing, creating, removing and executing files. From Linux 6.7 on it forbids TCP
//!   too, from 6.12 on connecting to abstract Unix sockets and signalling other processes.
//! - A seccomp filter refuses the system calls of networking, of starting programs and processes,
//!   of debugging other processes, and of kernel interfaces that rendering never needs
//!   (`io_uring`, `bpf`, mounting, modules, …), with `EPERM`. It is a list of what is refused
//!   rather than of what is allowed, so that it does not depend on which calls glibc, musl or a
//!   new Rust release make for allowed work.
//!
//! The sandbox is entered on the main thread before any other thread starts, so that every thread
//! inherits it. On kernels without Landlock or seccomp rendering goes on without them; on other
//! systems than Linux there is no sandbox.

use std::path::PathBuf;
use std::sync::OnceLock;

/// What the process may still do in the sandbox.
#[cfg_attr(not(target_os = "linux"), allow(dead_code, reason = "no sandbox there"))]
#[derive(Debug, Default)]
pub(crate) struct Policy {
    /// The files and directories (with everything below them) the process may read; `None`
    /// allows reading every file.
    pub read: Option<Vec<PathBuf>>,
    /// Directories in which the process may remove files: the output that rendering created, if
    /// it fails after all.
    pub remove: Vec<PathBuf>,
}

/// How much of the sandbox the system enforces.
#[cfg_attr(not(target_os = "linux"), allow(dead_code, reason = "no sandbox there"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// Landlock (with the rules the kernel knows) and the seccomp filter.
    On,
    /// Only one of Landlock and seccomp.
    Partial,
    /// None: turned off, or not supported by the system.
    Off,
}

impl Status {
    /// The name in the worker's `H` (docs/protocol.md).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Partial => "partial",
            Self::Off => "off",
        }
    }
}

static STATUS: OnceLock<Status> = OnceLock::new();

/// The status of the sandbox this process entered; `Off` if it entered none.
pub(crate) fn status() -> Status {
    STATUS.get().copied().unwrap_or(Status::Off)
}

/// Writes a line to standard error if `FAGUS_LOG` is `debug`.
pub(crate) fn debug(message: &str) {
    if std::env::var_os("FAGUS_LOG").is_some_and(|level| level == "debug") {
        eprintln!("fagus: {message}");
    }
}

/// Enters the sandbox for the calling thread and the threads it starts from now on. Never fails:
/// what the system does not support is left out, and said with `FAGUS_LOG=debug`.
pub(crate) fn enter(policy: &Policy) -> Status {
    let status = imp::enter(policy);
    // A process enters the sandbox once; the tests enter it on several threads.
    let _ = STATUS.set(status);
    status
}

/// Limits the memory (the address space, in MiB) and the processor time (in seconds) of the
/// process. Only on Linux.
pub(crate) fn limit(memory: Option<u64>, cpu: Option<u64>) -> std::io::Result<()> {
    imp::limit(memory, cpu)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::collections::BTreeMap;
    use std::io;

    use landlock::{
        ABI, Access, AccessFs, AccessNet, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
        path_beneath_rules,
    };
    use rlimit::Resource;
    use seccompiler::{
        BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
        SeccompRule, TargetArch,
    };

    use super::{Policy, Status, debug};

    /// The newest Landlock ABI whose rights the sandbox asks for; older kernels enforce the part
    /// they know.
    const ABI: ABI = ABI::V6;

    pub(super) fn enter(policy: &Policy) -> Status {
        let files = match landlock(policy) {
            Ok(status) => {
                debug(&format!("Landlock: {status:?}"));
                status
            }
            Err(error) => {
                debug(&format!("Landlock: not enforced ({error})"));
                RulesetStatus::NotEnforced
            }
        };
        let calls = match seccomp() {
            Ok(()) => {
                debug("seccomp: enforced");
                true
            }
            Err(error) => {
                debug(&format!("seccomp: not enforced ({error})"));
                false
            }
        };
        // Landlock of an older kernel enforces less (no truncating before 6.2, no TCP before 6.7,
        // no scopes before 6.12), but reading, writing and executing files from 5.13 on.
        match (files != RulesetStatus::NotEnforced, calls) {
            (true, true) => Status::On,
            (false, false) => Status::Off,
            _ => Status::Partial,
        }
    }

    pub(super) fn landlock(policy: &Policy) -> Result<RulesetStatus, landlock::RulesetError> {
        let read = AccessFs::ReadFile | AccessFs::ReadDir;
        let readable: Vec<std::path::PathBuf> = match &policy.read {
            Some(paths) => paths.iter().filter(|path| path.exists()).cloned().collect(),
            None => vec!["/".into()],
        };
        let removable: Vec<&std::path::PathBuf> = policy.remove.iter().filter(|path| path.is_dir()).collect();
        let status = Ruleset::default()
            .handle_access(AccessFs::from_all(ABI))?
            .handle_access(AccessNet::from_all(ABI))?
            .scope(Scope::from_all(ABI))?
            .create()?
            .add_rules(path_beneath_rules(&readable, read))?
            .add_rules(path_beneath_rules(removable, AccessFs::RemoveFile))?
            .restrict_self()?;
        Ok(status.ruleset)
    }

    /// The bit that marks the system calls of the x32 interface on x86-64.
    #[cfg(target_arch = "x86_64")]
    const X32: i64 = 0x4000_0000;

    /// The system calls refused with `EPERM`.
    fn refused() -> Vec<i64> {
        let calls = vec![
            // Networking: the document's resources come from the caller, never from the network.
            libc::SYS_socket,
            libc::SYS_socketpair,
            libc::SYS_connect,
            libc::SYS_bind,
            libc::SYS_listen,
            libc::SYS_accept,
            libc::SYS_accept4,
            // Starting programs; processes are refused below (`clone` without `CLONE_THREAD`).
            libc::SYS_execve,
            libc::SYS_execveat,
            // Reading or changing other processes.
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_pidfd_getfd,
            // Kernel interfaces that rendering never uses, which widen what an attacker reaches
            // (io_uring would also run calls past this filter).
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
            libc::SYS_bpf,
            libc::SYS_perf_event_open,
            libc::SYS_userfaultfd,
            libc::SYS_keyctl,
            libc::SYS_add_key,
            libc::SYS_request_key,
            libc::SYS_unshare,
            libc::SYS_setns,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_pivot_root,
            libc::SYS_chroot,
            libc::SYS_open_tree,
            libc::SYS_move_mount,
            libc::SYS_fsopen,
            libc::SYS_fsconfig,
            libc::SYS_fsmount,
            libc::SYS_fspick,
            libc::SYS_mount_setattr,
            libc::SYS_open_by_handle_at,
            libc::SYS_name_to_handle_at,
            libc::SYS_fanotify_init,
            libc::SYS_kexec_load,
            libc::SYS_init_module,
            libc::SYS_finit_module,
            libc::SYS_delete_module,
            libc::SYS_swapon,
            libc::SYS_swapoff,
            libc::SYS_reboot,
            libc::SYS_acct,
        ];
        #[cfg(target_arch = "x86_64")]
        let calls = {
            let mut calls = calls;
            calls.extend([libc::SYS_fork, libc::SYS_vfork]);
            // The same calls through the x32 interface, which numbers them with the bit `X32` set
            // and gives some of them numbers of their own (execve 520, ptrace 521,
            // process_vm_readv 539, process_vm_writev 540, execveat 545).
            let x32: Vec<i64> = calls
                .iter()
                .map(|call| call | X32)
                .chain([520, 521, 539, 540, 545].map(|call| call | X32))
                .chain([libc::SYS_clone | X32, libc::SYS_clone3 | X32])
                .collect();
            calls.extend(x32);
            calls
        };
        calls
    }

    pub(super) fn seccomp() -> Result<(), seccompiler::Error> {
        let arch = TargetArch::try_from(std::env::consts::ARCH)?;
        let mut rules: BTreeMap<i64, Vec<SeccompRule>> =
            refused().into_iter().map(|call| (call, Vec::new())).collect();
        // `clone` starts threads, which rendering uses, and processes, which it does not.
        rules.insert(
            libc::SYS_clone,
            vec![SeccompRule::new(vec![SeccompCondition::new(
                0,
                SeccompCmpArgLen::Qword,
                SeccompCmpOp::MaskedEq(libc::CLONE_THREAD as u64),
                0,
            )?])?],
        );
        let refuse: BpfProgram = SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Errno(libc::EPERM as u32),
            arch,
        )?
        .try_into()?;
        // `clone3` takes its flags in memory that a filter cannot read: `ENOSYS` makes glibc
        // start threads with `clone` instead.
        let clone3: BpfProgram = SeccompFilter::new(
            BTreeMap::from([(libc::SYS_clone3, Vec::new())]),
            SeccompAction::Allow,
            SeccompAction::Errno(libc::ENOSYS as u32),
            arch,
        )?
        .try_into()?;
        seccompiler::apply_filter(&refuse)?;
        seccompiler::apply_filter(&clone3)
    }

    pub(super) fn limit(memory: Option<u64>, cpu: Option<u64>) -> io::Result<()> {
        if let Some(mebibytes) = memory {
            lower(Resource::AS, mebibytes.saturating_mul(1 << 20), 0)?;
        }
        if let Some(seconds) = cpu {
            // The soft limit sends SIGXCPU, which ends the process; the hard one a second later
            // SIGKILL.
            lower(Resource::CPU, seconds, 1)?;
        }
        Ok(())
    }

    /// Lowers a limit to `soft` (and its hard limit to `soft + grace`); never raises one.
    fn lower(resource: Resource, soft: u64, grace: u64) -> io::Result<()> {
        let (current_soft, current_hard) = resource.get()?;
        let hard = soft.saturating_add(grace).min(current_hard);
        resource.set(soft.min(current_soft).min(hard), hard)
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::{Policy, Status, debug};

    pub(super) fn enter(_policy: &Policy) -> Status {
        debug("no sandbox on this system");
        Status::Off
    }

    pub(super) fn limit(memory: Option<u64>, cpu: Option<u64>) -> std::io::Result<()> {
        if memory.is_some() || cpu.is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "--memory-limit and --cpu-limit are only supported on Linux",
            ));
        }
        Ok(())
    }
}

/// The sandbox at work. Each test enters it on a thread of its own, which takes it along to the
/// threads it starts; the other threads of the tests stay outside. `FAGUS_REQUIRE_LANDLOCK=1`
/// (as in CI) fails the tests of files where the kernel has no Landlock, which they otherwise
/// skip.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::io::ErrorKind;
    use std::path::{Path, PathBuf};

    use super::*;

    fn tests_directory() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests")
    }

    /// Runs `check` on a new thread in the sandbox of `policy`, with whether Landlock enforces it.
    fn sandboxed<T: Send + 'static>(policy: Policy, check: impl FnOnce(bool) -> T + Send + 'static) -> T {
        std::thread::spawn(move || {
            let files = imp::landlock(&policy).expect("Landlock");
            let enforced = files != landlock::RulesetStatus::NotEnforced;
            if !enforced {
                assert!(
                    std::env::var_os("FAGUS_REQUIRE_LANDLOCK").is_none(),
                    "the kernel does not enforce Landlock"
                );
                eprintln!("Landlock is not enforced here: the checks of files are skipped");
            }
            imp::seccomp().expect("seccomp");
            check(enforced)
        })
        .join()
        .expect("the sandboxed thread")
    }

    #[test]
    fn refuses_the_network() {
        sandboxed(Policy::default(), |_| {
            let refused = |result: std::io::Result<()>| {
                assert_eq!(result.expect_err("refused").kind(), ErrorKind::PermissionDenied);
            };
            refused(std::net::TcpStream::connect(("127.0.0.1", 9)).map(drop));
            refused(std::net::TcpListener::bind(("127.0.0.1", 0)).map(drop));
            refused(std::net::UdpSocket::bind(("127.0.0.1", 0)).map(drop));
            refused(std::os::unix::net::UnixDatagram::unbound().map(drop));
            refused(std::os::unix::net::UnixStream::pair().map(drop));
        });
    }

    #[test]
    fn starts_threads_but_no_programs() {
        sandboxed(Policy::default(), |_| {
            let thread = std::thread::spawn(|| 6 * 7).join().expect("a thread");
            assert_eq!(thread, 42);
            for program in ["/bin/sh", "/usr/bin/env", "/proc/self/exe"] {
                let status = std::process::Command::new(program).arg("--version").status();
                assert!(status.is_err(), "{program} ran: {status:?}");
            }
        });
    }

    #[test]
    fn reads_only_the_allowed_files() {
        let tests = tests_directory();
        let scratch = std::env::temp_dir().join(format!("fagus-sandbox-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("a directory");
        let policy = Policy {
            read: Some(vec![tests.clone()]),
            remove: Vec::new(),
        };
        let inside = scratch.clone();
        sandboxed(policy, move |enforced| {
            assert!(std::fs::read(tests.join("fonts/Inter-LICENSE.txt")).is_ok());
            if !enforced {
                return;
            }
            let denied = |result: std::io::Result<()>, what: &str| {
                let error = result.expect_err(what);
                assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{what}: {error}");
            };
            denied(std::fs::read("/etc/passwd").map(drop), "/etc/passwd");
            denied(std::fs::read_dir("/etc").map(drop), "listing /etc");
            denied(std::fs::write(inside.join("out.pdf"), b"%PDF-"), "writing a file");
            denied(
                std::fs::write(tests.join("corpus/out.pdf"), b"%PDF-"),
                "writing where it reads",
            );
            denied(std::fs::create_dir(inside.join("sub")), "making a directory");
        });
        std::fs::remove_dir_all(scratch).expect("removed");
    }

    /// The pages of the corpus, rendered with their files and fonts.
    fn render_corpus(engine: &fagus::Engine, tests: &Path) -> Vec<(PathBuf, Vec<u8>, Vec<String>)> {
        let mut pages: Vec<PathBuf> = ["corpus", "corpus/invoice"]
            .iter()
            .flat_map(|directory| std::fs::read_dir(tests.join(directory)).expect("the corpus"))
            .map(|entry| entry.expect("an entry").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "html"))
            .collect();
        pages.sort();
        pages
            .into_iter()
            .map(|page| {
                let html = std::fs::read_to_string(&page).expect("the page");
                let document = fagus::Document::html(html)
                    .base_directory(page.parent().expect("a directory"))
                    .root(tests)
                    .created(crate::date::parse("2026-01-01T12:00:00Z").expect("a date"));
                let output = engine
                    .render_with_diagnostics(&document)
                    .unwrap_or_else(|failure| panic!("{}: {}", page.display(), failure.error));
                let codes = output
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code.to_owned())
                    .collect();
                (page, output.pdf, codes)
            })
            .collect()
    }

    #[test]
    fn renders_the_corpus_as_outside() {
        let tests = tests_directory();
        let mut engine = fagus::Engine::new();
        engine.add_font_dir(tests.join("fonts")).expect("fonts");
        let outside = render_corpus(&engine, &tests);
        let policy = Policy {
            read: Some(vec![tests.clone()]),
            remove: Vec::new(),
        };
        // A new engine, so that no image or style sheet comes from the cache of the first.
        let mut engine = fagus::Engine::new();
        engine.add_font_dir(tests.join("fonts")).expect("fonts");
        let inside = sandboxed(policy, move |_| render_corpus(&engine, &tests));
        assert_eq!(inside.len(), outside.len());
        for (inside, outside) in inside.iter().zip(&outside) {
            assert_eq!(inside.2, outside.2, "the diagnostics of {}", inside.0.display());
            assert!(inside.1 == outside.1, "the bytes of {}", inside.0.display());
        }
    }
}
