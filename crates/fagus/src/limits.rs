//! The memory budget and the time limit of a document (see [`Limits`]).

use std::time::Duration;

use fagus_css::{Diagnostic, Severity};
use fagus_dom::budget::Exceeded;

use crate::{Error, Failure};

/// How much memory and time one document may take, all its parts together: a document that
/// needs more ends with [`Error::MemoryBudget`] or [`Error::TimeLimit`] instead of a PDF, and
/// the engine goes on with the next one. The limits only decide whether a document is rendered,
/// never what it looks like.
///
/// The memory budget counts what the large parts of a document hold, as an estimate rather than
/// what the allocator counts: the document tree, computed styles, layout boxes and fragments,
/// table cells (with the rows each one spans), display lists, decoded images (four bytes a
/// pixel), pixels rasterized for filters, shadows and masks, fonts that documents bring
/// (unpacked) and the bytes of resources (each content once, however many URLs it has). The
/// process takes more than that: the engine's own fonts and caches, and what the allocator
/// keeps. The time limit is wall-clock time from the call to [`crate::Engine::render`] on,
/// including the time a [`crate::ResourceLoader`] takes; the engine looks at the clock for each
/// element, box, page, image and resource, so a document stops shortly after its time is up.
///
/// ```
/// use std::time::Duration;
/// use fagus::{Document, Engine, Limits};
///
/// let mut engine = Engine::new();
/// engine.set_limits(Limits::new().memory(Some(1 << 30)).time(Some(Duration::from_secs(30))));
/// let output = engine.render(&Document::html("<p>Rechnung</p>")).unwrap();
/// # assert_eq!(output.pages, 1);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    memory: Option<u64>,
    time: Option<Duration>,
}

impl Limits {
    /// The memory budget of a document unless another is set: 2 GiB.
    pub const DEFAULT_MEMORY: u64 = 2 << 30;

    /// The default limits: a memory budget of [`Self::DEFAULT_MEMORY`] and no time limit.
    pub const fn new() -> Self {
        Self {
            memory: Some(Self::DEFAULT_MEMORY),
            time: None,
        }
    }

    /// No memory budget and no time limit: only the limits of each part (pixels per image,
    /// pages, nesting) apply.
    pub const fn unlimited() -> Self {
        Self {
            memory: None,
            time: None,
        }
    }

    /// Sets the memory budget, in bytes (none if `None`).
    #[must_use]
    pub const fn memory(mut self, bytes: Option<u64>) -> Self {
        self.memory = bytes;
        self
    }

    /// Sets the time limit (none if `None`).
    #[must_use]
    pub const fn time(mut self, limit: Option<Duration>) -> Self {
        self.time = limit;
        self
    }

    /// The memory budget in bytes, if any.
    pub const fn memory_budget(&self) -> Option<u64> {
        self.memory
    }

    /// The time limit, if any.
    pub const fn time_limit(&self) -> Option<Duration> {
        self.time
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::new()
    }
}

/// A size in bytes for messages, in MiB or GiB.
pub(crate) fn size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GiB", bytes as f64 / f64::from(1u32 << 30))
    } else {
        format!("{} MiB", bytes.div_ceil(1 << 20))
    }
}

/// The parts that held most of a budget that ran out, for messages.
pub(crate) fn held(held: &[(&'static str, u64)]) -> String {
    held.iter()
        .take(3)
        .map(|(kind, bytes)| format!("{kind} {}", size(*bytes)))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Failure {
    /// The failure of a document whose budget or time ran out, with a diagnostic that says so.
    pub(crate) fn exceeded(exceeded: Exceeded) -> Self {
        let (error, code) = match exceeded {
            Exceeded::Memory { budget, held } => (
                Error::MemoryBudget {
                    budget,
                    held: held
                        .into_iter()
                        .map(|(kind, bytes)| (kind.name(), bytes))
                        .collect(),
                },
                "limit.memory",
            ),
            Exceeded::Time { limit } => (Error::TimeLimit { limit }, "limit.time"),
        };
        Self {
            diagnostics: vec![Diagnostic::new(
                Severity::Error,
                code,
                error.to_string(),
                "document",
            )],
            error,
        }
    }
}
