//! The memory budget and the time limit of one document.
//!
//! Each part of the engine has limits of its own (pixels per image, nesting, pages); the budget
//! bounds the document as a whole. While a document renders, its [`Budget`] is the current one of
//! the thread that renders it ([`run`]). The crates charge what the big consumers hold to it, an
//! estimate in bytes rather than what the allocator counts: the document tree, computed styles,
//! layout boxes and fragments, table cells, display lists, decoded images and rasterized pixels, fonts and the
//! bytes of resources. Some of it is held for the rest of the render ([`charge`]), some only while
//! the part that holds it lives ([`hold`], [`Held`]). They also look at the clock at points
//! spread through the work (per element, box, page and image; [`check_time`]).
//!
//! Where the budget or the time runs out, the render ends: [`run`] returns [`Exceeded`]. The
//! work stops by unwinding to [`run`] with [`std::panic::resume_unwind`], which does not call the
//! panic hook: nothing is printed, and the fuzz targets, whose hook aborts on panics, see no
//! panic. Neither the budget nor the clock changes what a document that stays within them
//! renders: they only decide whether it is rendered at all.
//!
//! Without a current budget (outside [`run`], or on the threads that compress the PDF), charges
//! and checks do nothing.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// What memory is charged for, to tell in the error what took it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The nodes of parsed documents and their markup.
    Document,
    /// Computed styles.
    Styles,
    /// Layout boxes and fragments.
    Layout,
    /// The cells and rows of tables, with the rows each cell spans.
    Tables,
    /// The display lists of painted pages.
    Painting,
    /// Decoded images, four bytes a pixel: held for the images that Fagus decodes itself, room
    /// needed for the others, which krilla decodes one after another as it writes the PDF.
    Images,
    /// Pixels rasterized for filters, shadows, masks and blurred text.
    Pixels,
    /// The data of fonts that documents bring (`@font-face`), unpacked.
    Fonts,
    /// The bytes of resources (style sheets, images, fonts, frames), each content once.
    Resources,
}

const KINDS: [Kind; 9] = [
    Kind::Document,
    Kind::Styles,
    Kind::Layout,
    Kind::Tables,
    Kind::Painting,
    Kind::Images,
    Kind::Pixels,
    Kind::Fonts,
    Kind::Resources,
];

impl Kind {
    /// What it is, for messages.
    pub fn name(self) -> &'static str {
        match self {
            Self::Document => "document tree",
            Self::Styles => "styles",
            Self::Layout => "layout",
            Self::Tables => "table cells",
            Self::Painting => "display lists",
            Self::Images => "decoded images",
            Self::Pixels => "rasterized pixels",
            Self::Fonts => "fonts",
            Self::Resources => "resources",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Why a render was ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exceeded {
    /// The document needed more memory than its budget, in bytes: what it held then, by kind,
    /// the most first (kinds that held nothing left out).
    Memory { budget: u64, held: Vec<(Kind, u64)> },
    /// The document took longer than its time limit.
    Time { limit: Duration },
}

/// The memory budget and the deadline of one document, and what it holds.
#[derive(Debug)]
pub struct Budget {
    memory: Option<u64>,
    time: Option<Duration>,
    deadline: Option<Instant>,
    held: [AtomicU64; KINDS.len()],
    total: AtomicU64,
    peak: AtomicU64,
    ticks: AtomicU32,
    /// Why the render is to end, noted where it cannot end at once (see [`defer`]).
    noted: Mutex<Option<Exceeded>>,
}

impl Budget {
    /// A budget of `memory` bytes (none if `None`) and a deadline `time` after `start` (none if
    /// `None`).
    pub fn new(memory: Option<u64>, time: Option<Duration>, start: Instant) -> Arc<Self> {
        Arc::new(Self {
            memory,
            time,
            deadline: time.and_then(|time| start.checked_add(time)),
            held: Default::default(),
            total: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            ticks: AtomicU32::new(0),
            noted: Mutex::new(None),
        })
    }

    /// The most the document held at once so far, in bytes.
    pub fn peak(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }

    /// What the document holds now of `kind`, in bytes.
    pub fn held(&self, kind: Kind) -> u64 {
        self.held[kind.index()].load(Ordering::Relaxed)
    }

    fn add(&self, kind: Kind, bytes: u64) -> u64 {
        self.held[kind.index()].fetch_add(bytes, Ordering::Relaxed);
        let total = self
            .total
            .fetch_add(bytes, Ordering::Relaxed)
            .saturating_add(bytes);
        self.peak.fetch_max(total, Ordering::Relaxed);
        total
    }

    /// Releases `bytes` of `kind`, which were added before.
    fn release(&self, kind: Kind, bytes: u64) {
        self.held[kind.index()].fetch_sub(bytes, Ordering::Relaxed);
        self.total.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Ends the render if `total` bytes are more than the budget; `more` is needed beside what
    /// is held.
    fn check_memory(&self, total: u64, more: Option<(Kind, u64)>) {
        if let Some(budget) = self.memory
            && total > budget
        {
            let mut held: Vec<(Kind, u64)> = KINDS
                .iter()
                .map(|&kind| {
                    let more = more
                        .filter(|&(needed, _)| needed == kind)
                        .map_or(0, |(_, bytes)| bytes);
                    (kind, self.held(kind).saturating_add(more))
                })
                .filter(|&(_, bytes)| bytes > 0)
                .collect();
            held.sort_by_key(|&(_, bytes)| std::cmp::Reverse(bytes));
            self.exceed(Exceeded::Memory { budget, held });
        }
    }

    fn check_deadline(&self) {
        if let (Some(deadline), Some(limit)) = (self.deadline, self.time)
            && Instant::now() >= deadline
        {
            self.exceed(Exceeded::Time { limit });
        }
    }

    /// Ends the render: unwinds to [`run`], without the panic hook. Where the render cannot end
    /// at once ([`defer`]), and while the thread unwinds already (where a second panic would
    /// abort the process), the reason is noted instead, the first one only.
    fn exceed(&self, exceeded: Exceeded) {
        if DEFERRED.get() == 0 && !std::thread::panicking() {
            resume_unwind(Box::new(exceeded));
        }
        self.noted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(exceeded);
    }

    /// Whether a reason to end the render is noted.
    fn spent(&self) -> bool {
        self.noted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }
}

thread_local! {
    /// The budget of the document this thread renders.
    static CURRENT: RefCell<Option<Arc<Budget>>> = const { RefCell::new(None) };
    /// How many [`Deferred`] guards this thread holds.
    static DEFERRED: Cell<u32> = const { Cell::new(0) };
}

/// The budget of the document this thread renders, if any.
fn current() -> Option<Arc<Budget>> {
    CURRENT
        .try_with(|current| current.borrow().clone())
        .ok()
        .flatten()
}

/// Calls `with` with the budget of this thread, if there is one, without cloning it.
fn with_current(with: impl FnOnce(&Budget)) {
    let _ = CURRENT.try_with(|current| {
        if let Some(budget) = &*current.borrow() {
            with(budget);
        }
    });
}

/// Notes a budget or time that runs out instead of ending the render at once, until the guard is
/// dropped: for work that must not be unwound through, such as drawing onto a page of krilla,
/// whose surfaces assert that they are finished when dropped. The work calls [`settle`] where it
/// can end, and can skip what is costly once [`spent`] says the render is to end anyway.
pub fn defer() -> Deferred {
    DEFERRED.set(DEFERRED.get() + 1);
    Deferred(())
}

/// The guard of [`defer`].
#[derive(Debug)]
pub struct Deferred(());

impl Drop for Deferred {
    fn drop(&mut self) {
        DEFERRED.set(DEFERRED.get().saturating_sub(1));
    }
}

/// Ends the render if a reason to is noted (see [`defer`]). Only where unwinding is safe.
pub fn settle() {
    if std::thread::panicking() {
        return;
    }
    if let Some(budget) = current()
        && let Some(exceeded) = budget.noted.lock().unwrap_or_else(PoisonError::into_inner).take()
    {
        resume_unwind(Box::new(exceeded));
    }
}

/// Whether the render is to end (see [`defer`]): what it would still make is of no use.
pub fn spent() -> bool {
    current().is_some_and(|budget| budget.spent())
}

/// Sets the budget of this thread, and takes it away again when dropped (also while unwinding).
struct Installed(Option<Arc<Budget>>);

impl Drop for Installed {
    fn drop(&mut self) {
        let previous = self.0.take();
        let _ = CURRENT.try_with(|current| *current.borrow_mut() = previous);
    }
}

/// Runs `work` with `budget` as the budget of this thread: `Err` if the budget or the time ran
/// out. Inside the work of another budget (the document of a frame), the work counts for that
/// one, and `budget` is not used. Panics other than the budget's go on unwinding.
pub fn run<T>(budget: Arc<Budget>, work: impl FnOnce() -> T) -> Result<T, Exceeded> {
    if current().is_some() {
        return Ok(work());
    }
    let previous = CURRENT
        .try_with(|current| current.borrow_mut().replace(budget))
        .ok()
        .flatten();
    let installed = Installed(previous);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let value = work();
        settle();
        value
    }));
    drop(installed);
    match result {
        Ok(value) => Ok(value),
        Err(payload) => match payload.downcast::<Exceeded>() {
            Ok(exceeded) => Err(*exceeded),
            Err(payload) => resume_unwind(payload),
        },
    }
}

/// Whether `payload` (of a caught panic) is a budget that ran out: such a panic is not to be
/// caught on the way to [`run`].
pub fn is_exceeded(payload: &(dyn std::any::Any + Send)) -> bool {
    payload.is::<Exceeded>()
}

/// Charges `bytes` of `kind` for the rest of the render; ends it if the budget is spent.
pub fn charge(kind: Kind, bytes: u64) {
    with_current(|budget| {
        let total = budget.add(kind, bytes);
        budget.check_memory(total, None);
    });
}

/// Ends the render if the budget has no room for `bytes` more of `kind`, without charging them:
/// before reading or making something that large, which is let go of soon.
pub fn ensure_room(kind: Kind, bytes: u64) {
    with_current(|budget| {
        budget.check_memory(
            budget.total.load(Ordering::Relaxed).saturating_add(bytes),
            Some((kind, bytes)),
        );
    });
}

/// Charges `bytes` of `kind` while the returned [`Held`] lives; ends the render if the budget is
/// spent.
pub fn hold(kind: Kind, bytes: u64) -> Held {
    let mut held = Held {
        budget: current(),
        kind,
        bytes: 0,
    };
    held.add(bytes);
    held
}

/// Looks at the clock now and then (every 16th call): ends the render once its time is up. For
/// the small steps (elements, boxes).
pub fn check_time() {
    with_current(|budget| {
        if budget.deadline.is_some() && budget.ticks.fetch_add(1, Ordering::Relaxed).is_multiple_of(16) {
            budget.check_deadline();
        }
    });
}

/// Looks at the clock: ends the render once its time is up. For the large steps (pages, images,
/// resources).
pub fn check_time_now() {
    with_current(Budget::check_deadline);
}

/// Memory charged to a budget while it lives (see [`hold`]). A clone holds nothing.
#[derive(Debug)]
pub struct Held {
    budget: Option<Arc<Budget>>,
    kind: Kind,
    bytes: u64,
}

impl Held {
    /// Nothing held.
    pub const fn none(kind: Kind) -> Self {
        Self {
            budget: None,
            kind,
            bytes: 0,
        }
    }

    /// Charges `bytes` more; ends the render if the budget is spent (on the thread that renders
    /// with the budget; elsewhere they are only counted).
    pub fn add(&mut self, bytes: u64) {
        let Some(budget) = &self.budget else {
            return;
        };
        self.bytes = self.bytes.saturating_add(bytes);
        let total = budget.add(self.kind, bytes);
        if current().is_some_and(|current| Arc::ptr_eq(&current, budget)) {
            budget.check_memory(total, None);
        }
    }
}

impl Default for Held {
    fn default() -> Self {
        Self::none(Kind::Layout)
    }
}

impl Clone for Held {
    fn clone(&self) -> Self {
        Self::none(self.kind)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(budget) = &self.budget {
            budget.release(self.kind, self.bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ends_the_work_when_the_memory_runs_out() {
        let budget = Budget::new(Some(1000), None, Instant::now());
        let result = run(Arc::clone(&budget), || {
            charge(Kind::Resources, 600);
            {
                let _held = hold(Kind::Pixels, 300);
            }
            // Released again: there is room.
            charge(Kind::Fonts, 300);
            charge(Kind::Images, 200);
            unreachable!("over the budget");
        });
        assert_eq!(
            result,
            Err(Exceeded::Memory {
                budget: 1000,
                held: vec![(Kind::Resources, 600), (Kind::Fonts, 300), (Kind::Images, 200)],
            })
        );
        assert_eq!(budget.peak(), 1100);
        // The budget is gone with the work.
        charge(Kind::Images, u64::MAX / 2);
    }

    #[test]
    fn ends_the_work_when_the_time_is_up() {
        let start = Instant::now()
            .checked_sub(Duration::from_secs(2))
            .expect("a past instant");
        let budget = Budget::new(None, Some(Duration::from_secs(1)), start);
        let result: Result<(), _> = run(budget, || {
            loop {
                check_time();
            }
        });
        assert_eq!(
            result,
            Err(Exceeded::Time {
                limit: Duration::from_secs(1)
            })
        );
    }

    #[test]
    fn counts_nested_work_for_the_outer_budget() {
        let outer = Budget::new(Some(100), None, Instant::now());
        let inner = Budget::new(None, None, Instant::now());
        let result = run(outer, || run(inner, || charge(Kind::Document, 200)));
        assert!(matches!(result, Err(Exceeded::Memory { budget: 100, .. })));
    }

    #[test]
    fn lets_other_panics_through() {
        let budget = Budget::new(None, None, Instant::now());
        let caught = catch_unwind(|| run(budget, || panic!("a bug")));
        assert!(caught.is_err());
        // And the thread has no budget after it.
        assert!(current().is_none());
    }
}
