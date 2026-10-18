use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;

use cloakframe_core::coverage::CoverageEvidence;
use cloakframe_core::detection::Detection;
use cloakframe_core::item::{Item, ItemId, ItemState, PublishBasis, PublishPermit, ReviewMode};
use cloakframe_core::review::Digest;

use crate::cancel::CancelToken;
use crate::detect::DetectorHandle;
use crate::events::{Event, ProgressBoard, ProgressReporter};
use crate::ordered::process_ordered;

const EVENT_CAPACITY: usize = 64;

#[derive(Debug)]
pub enum StepError {
    Cancelled,
    Failed(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Failed(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for StepError {}

pub struct ItemContext<'a> {
    pub item: ItemId,
    pub input: &'a Path,
    /// Where the output is published. Its folder holds the temp output while rendering.
    pub destination: &'a Path,
    pub cancel: &'a CancelToken,
    pub detector: &'a DetectorHandle,
    pub progress: ProgressReporter,
}

/// The media-specific steps of one item. The runner owns the state machine and the publish
/// permit; a processor only does the work and reports what it could not cover.
pub trait ItemProcessor: Send + Sync + 'static {
    fn analyze(&self, ctx: &ItemContext<'_>) -> Result<Analysis, StepError>;
    /// Renders the output to a temporary file next to `ctx.destination` and verifies it.
    fn render(&self, ctx: &ItemContext<'_>, analysis: &Analysis) -> Result<(), StepError>;
    fn publish(&self, ctx: &ItemContext<'_>, permit: &PublishPermit) -> Result<(), StepError>;
    /// What besides the input decides the results. A stored review is bound to it, so changing
    /// models or settings invalidates earlier review decisions.
    fn fingerprint(&self) -> Fingerprint;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    pub models: Digest,
    pub settings: Digest,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Analysis {
    pub detections: Vec<Detection>,
    pub evidence: Vec<CoverageEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    ExportWithWarnings,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Start {
    /// `needs_review` holds the item for review even in automatic mode, for example because its
    /// output name is taken.
    Fresh { needs_review: bool },
    /// Resumes a stored analysis with the reviewer's decision; evidence carries the resolutions.
    Reviewed {
        decision: Decision,
        analysis: Analysis,
    },
}

#[derive(Debug, Clone)]
pub struct JobItem {
    pub id: ItemId,
    pub input: PathBuf,
    pub destination: PathBuf,
    pub start: Start,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub items: Vec<JobItem>,
    pub review: ReviewMode,
    pub workers: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemOutcome {
    pub item: ItemId,
    pub input: PathBuf,
    pub destination: PathBuf,
    pub state: ItemState,
    pub analysis: Analysis,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunReport {
    pub items: Vec<ItemOutcome>,
}

impl RunReport {
    pub fn count(&self, matches: impl Fn(ItemState) -> bool) -> usize {
        self.items.iter().filter(|o| matches(o.state)).count()
    }

    /// Published items that count as a normal completion; exports with warnings do not.
    pub fn completed(&self) -> usize {
        self.count(|s| matches!(s, ItemState::Published(b) if b.counts_as_normal_completion()))
    }

    pub fn published_with(&self, basis: PublishBasis) -> usize {
        self.count(|s| s == ItemState::Published(basis))
    }
}

pub struct RunHandle {
    events: mpsc::Receiver<Event>,
    progress: ProgressBoard,
    cancel: CancelToken,
    items: BTreeMap<ItemId, CancelToken>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RunHandle {
    pub fn events(&self) -> &mpsc::Receiver<Event> {
        &self.events
    }

    pub fn progress(&self) -> &ProgressBoard {
        &self.progress
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Returns false when the run has no such item.
    pub fn cancel_item(&self, item: ItemId) -> bool {
        self.items.get(&item).map(CancelToken::cancel).is_some()
    }

    /// Drains the remaining events and returns the final report.
    pub fn wait(mut self) -> RunReport {
        let mut report = None;
        for event in self.events.iter() {
            if let Event::RunFinished(r) = event {
                report = Some(r);
            }
        }
        if let Some(thread) = self.thread.take()
            && let Err(panic) = thread.join()
        {
            std::panic::resume_unwind(panic);
        }
        report.expect("the run thread always sends RunFinished")
    }
}

pub fn start(
    job: Job,
    processor: Arc<dyn ItemProcessor>,
    detector: DetectorHandle,
) -> std::io::Result<RunHandle> {
    let (sender, events) = mpsc::sync_channel(EVENT_CAPACITY);
    let progress = ProgressBoard::default();
    let cancel = CancelToken::new();
    let items: BTreeMap<ItemId, CancelToken> =
        job.items.iter().map(|i| (i.id, cancel.child())).collect();

    let run = Runner {
        job,
        processor,
        detector,
        events: sender,
        progress: progress.clone(),
        cancel: cancel.clone(),
        items: items.clone(),
    };
    let thread = thread::Builder::new()
        .name("cloakframe-run".into())
        .spawn(move || run.run())?;
    Ok(RunHandle {
        events,
        progress,
        cancel,
        items,
        thread: Some(thread),
    })
}

struct Runner {
    job: Job,
    processor: Arc<dyn ItemProcessor>,
    detector: DetectorHandle,
    events: mpsc::SyncSender<Event>,
    progress: ProgressBoard,
    cancel: CancelToken,
    items: BTreeMap<ItemId, CancelToken>,
}

impl Runner {
    fn emit(&self, event: Event) {
        // A dropped receiver means nobody is listening; the run still finishes its items.
        let _ = self.events.send(event);
    }

    fn run(self) {
        for job_item in &self.job.items {
            self.emit(Event::ItemState {
                item: job_item.id,
                state: initial(job_item).state(),
            });
        }
        let mut outcomes: Vec<Option<ItemOutcome>> = vec![None; self.job.items.len()];
        let result = process_ordered(
            self.job.items.len(),
            self.job.workers,
            self.job.workers.max(1) * 2,
            &self.cancel,
            |i| Ok::<_, std::convert::Infallible>(self.process(&self.job.items[i])),
            |i, outcome| {
                outcomes[i] = Some(outcome);
                Ok(())
            },
        );
        match result {
            Ok(()) => {}
            Err(never) => match never {},
        }
        let items = self
            .job
            .items
            .iter()
            .zip(outcomes)
            .map(|(job_item, outcome)| {
                outcome.unwrap_or_else(|| {
                    // Never started because the run was cancelled.
                    let mut item = initial(job_item);
                    item.cancel().expect("unstarted items can be cancelled");
                    self.emit(Event::ItemState {
                        item: job_item.id,
                        state: item.state(),
                    });
                    ItemOutcome {
                        item: job_item.id,
                        input: job_item.input.clone(),
                        destination: job_item.destination.clone(),
                        state: item.state(),
                        analysis: match &job_item.start {
                            Start::Fresh { .. } => Analysis::default(),
                            Start::Reviewed { analysis, .. } => analysis.clone(),
                        },
                        failure: None,
                    }
                })
            })
            .collect();
        self.emit(Event::RunFinished(RunReport { items }));
    }

    fn process(&self, job_item: &JobItem) -> ItemOutcome {
        let id = job_item.id;
        let ctx = ItemContext {
            item: id,
            input: &job_item.input,
            destination: &job_item.destination,
            cancel: &self.items[&id],
            detector: &self.detector,
            progress: self.progress.reporter(id),
        };
        let mut item = initial(job_item);
        let mut analysis = match &job_item.start {
            Start::Fresh { .. } => Analysis::default(),
            Start::Reviewed { analysis, .. } => analysis.clone(),
        };
        let result = self.drive(&ctx, &job_item.start, &mut item, &mut analysis);
        let failure = match result {
            Ok(()) => None,
            Err(StepError::Cancelled) => {
                item.cancel().expect("non-terminal items can be cancelled");
                None
            }
            Err(StepError::Failed(e)) => {
                item.fail().expect("non-terminal items can fail");
                Some(e.to_string())
            }
        };
        if failure.is_some() || item.state() == ItemState::Cancelled {
            self.emit(Event::ItemState {
                item: id,
                state: item.state(),
            });
        }
        ItemOutcome {
            item: id,
            input: job_item.input.clone(),
            destination: job_item.destination.clone(),
            state: item.state(),
            analysis,
            failure,
        }
    }

    fn drive(
        &self,
        ctx: &ItemContext<'_>,
        start: &Start,
        item: &mut Item,
        analysis: &mut Analysis,
    ) -> Result<(), StepError> {
        let checkpoint = || {
            if ctx.cancel.is_cancelled() {
                Err(StepError::Cancelled)
            } else {
                Ok(())
            }
        };
        let moved = |item: &Item| {
            self.emit(Event::ItemState {
                item: item.id(),
                state: item.state(),
            })
        };

        checkpoint()?;
        match start {
            Start::Fresh { needs_review } => {
                *analysis = self.processor.analyze(ctx)?;
                item.finish_analysis().expect("queued item");
                moved(item);
                let mode = if *needs_review {
                    ReviewMode::Required
                } else {
                    self.job.review
                };
                item.route(mode, &analysis.evidence).expect("analyzed item");
                moved(item);
                if item.state() == ItemState::NeedsReview {
                    return Ok(());
                }
            }
            Start::Reviewed {
                decision: Decision::Approve,
                ..
            } => {
                item.approve_review(&analysis.evidence)
                    .map_err(|e| StepError::Failed(e.into()))?;
                moved(item);
            }
            Start::Reviewed {
                decision: Decision::ExportWithWarnings,
                ..
            } => {
                item.export_with_warnings().expect("item awaiting review");
                moved(item);
            }
        }

        checkpoint()?;
        item.start_render().expect("item ready for render");
        moved(item);
        self.processor.render(ctx, analysis)?;
        item.mark_verified().expect("rendering item");
        moved(item);

        checkpoint()?;
        let permit = PublishPermit::issue(item, &analysis.evidence)
            .map_err(|refusal| StepError::Failed(refusal.into()))?;
        self.processor.publish(ctx, &permit)?;
        item.publish(permit).expect("permit issued for this item");
        moved(item);
        Ok(())
    }
}

fn initial(job_item: &JobItem) -> Item {
    match job_item.start {
        Start::Fresh { .. } => Item::new(job_item.id),
        Start::Reviewed { .. } => Item::awaiting_review(job_item.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{DetectError, Detector, Found, Rgb8Image};
    use cloakframe_core::coverage::Problem;
    use cloakframe_core::detection::Thresholds;
    use std::sync::Mutex;
    use std::time::Duration;

    struct NoFaces;

    impl Detector for NoFaces {
        fn detect(&mut self, _: &Rgb8Image, _: &Thresholds) -> Result<Found, DetectError> {
            Ok(Found::default())
        }
    }

    #[derive(Debug)]
    struct Broken(&'static str);

    impl fmt::Display for Broken {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Broken {}

    /// Behaves according to the input's file name: `ok`, `gap` (reports an unresolved gap),
    /// `fail-analyze`, `fail-render`, `fail-publish`, `slow` (works until cancelled).
    #[derive(Default)]
    struct Scripted {
        published: Mutex<Vec<(ItemId, PublishBasis)>>,
    }

    fn name<'a>(ctx: &'a ItemContext<'a>) -> &'a str {
        ctx.input.to_str().unwrap()
    }

    impl ItemProcessor for Scripted {
        fn analyze(&self, ctx: &ItemContext<'_>) -> Result<Analysis, StepError> {
            ctx.detector
                .detect(
                    Rgb8Image {
                        width: 1,
                        height: 1,
                        data: vec![0; 3],
                    },
                    Thresholds::default(),
                )
                .map_err(|e| StepError::Failed(e.into()))?;
            match name(ctx) {
                "fail-analyze" => Err(StepError::Failed(Box::new(Broken("decode failed")))),
                "gap" => Ok(Analysis {
                    detections: Vec::new(),
                    evidence: vec![gap()],
                }),
                "slow" => {
                    for done in 0.. {
                        if ctx.cancel.is_cancelled() {
                            return Err(StepError::Cancelled);
                        }
                        ctx.progress.set(done, 1_000_000);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    unreachable!()
                }
                _ => Ok(Analysis::default()),
            }
        }

        fn render(&self, ctx: &ItemContext<'_>, _: &Analysis) -> Result<(), StepError> {
            ctx.progress.set(1, 1);
            match name(ctx) {
                "fail-render" => Err(StepError::Failed(Box::new(Broken("encode failed")))),
                _ => Ok(()),
            }
        }

        fn fingerprint(&self) -> Fingerprint {
            Fingerprint {
                models: Digest([0; 32]),
                settings: Digest([0; 32]),
            }
        }

        fn publish(&self, ctx: &ItemContext<'_>, permit: &PublishPermit) -> Result<(), StepError> {
            assert_eq!(permit.item(), ctx.item);
            match name(ctx) {
                "fail-publish" => Err(StepError::Failed(Box::new(Broken("name taken")))),
                _ => {
                    self.published
                        .lock()
                        .unwrap()
                        .push((permit.item(), permit.basis()));
                    Ok(())
                }
            }
        }
    }

    fn gap() -> CoverageEvidence {
        CoverageEvidence::unresolved(Problem::UnresolvedGap {
            track: 1,
            first: 30,
            last: 54,
        })
    }

    fn job(inputs: &[&str], review: ReviewMode, workers: usize) -> Job {
        Job {
            items: inputs
                .iter()
                .enumerate()
                .map(|(i, name)| JobItem {
                    id: ItemId(i as u64),
                    input: PathBuf::from(name),
                    destination: PathBuf::from(format!("out/{i}")),
                    start: Start::Fresh {
                        needs_review: false,
                    },
                })
                .collect(),
            review,
            workers,
        }
    }

    fn run(job: Job) -> (RunReport, Vec<(ItemId, PublishBasis)>, Vec<Event>) {
        let processor = Arc::new(Scripted::default());
        let detector = DetectorHandle::spawn(Box::new(NoFaces)).unwrap();
        let handle = start(job, processor.clone(), detector).unwrap();
        let events: Vec<Event> = handle.events().iter().collect();
        let Some(Event::RunFinished(report)) = events.last().cloned() else {
            panic!("last event is not RunFinished");
        };
        let published = processor.published.lock().unwrap().clone();
        (report, published, events)
    }

    fn states_of(events: &[Event], item: ItemId) -> Vec<ItemState> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::ItemState { item: i, state } if *i == item => Some(*state),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn automatic_mode_publishes_clean_items_and_holds_problems_for_review() {
        let (report, published, events) = run(job(&["ok", "gap", "ok"], ReviewMode::Off, 2));
        assert_eq!(
            report.items.iter().map(|o| o.state).collect::<Vec<_>>(),
            [
                ItemState::Published(PublishBasis::AutomaticUnreviewed),
                ItemState::NeedsReview,
                ItemState::Published(PublishBasis::AutomaticUnreviewed),
            ]
        );
        assert_eq!(report.completed(), 2);
        assert_eq!(report.items[1].analysis.evidence, [gap()]);
        let mut published = published;
        published.sort_by_key(|p| p.0);
        assert_eq!(
            published,
            [
                (ItemId(0), PublishBasis::AutomaticUnreviewed),
                (ItemId(2), PublishBasis::AutomaticUnreviewed)
            ]
        );
        assert_eq!(
            states_of(&events, ItemId(0)),
            [
                ItemState::Queued,
                ItemState::Analyzed,
                ItemState::ReadyForRender(PublishBasis::AutomaticUnreviewed),
                ItemState::Rendering(PublishBasis::AutomaticUnreviewed),
                ItemState::Verified(PublishBasis::AutomaticUnreviewed),
                ItemState::Published(PublishBasis::AutomaticUnreviewed),
            ]
        );
    }

    #[test]
    fn review_mode_publishes_nothing_before_review() {
        let (report, published, _) = run(job(&["ok", "gap"], ReviewMode::Required, 2));
        assert_eq!(report.count(|s| s == ItemState::NeedsReview), 2);
        assert!(published.is_empty());
    }

    #[test]
    fn a_failing_step_fails_only_that_item() {
        let (report, published, events) = run(job(
            &["fail-analyze", "fail-render", "fail-publish", "ok"],
            ReviewMode::Off,
            1,
        ));
        let failures: Vec<_> = report.items.iter().map(|o| o.failure.as_deref()).collect();
        assert_eq!(
            failures,
            [
                Some("decode failed"),
                Some("encode failed"),
                Some("name taken"),
                None
            ]
        );
        assert_eq!(report.count(|s| s == ItemState::Failed), 3);
        assert_eq!(published, [(ItemId(3), PublishBasis::AutomaticUnreviewed)]);
        assert_eq!(
            states_of(&events, ItemId(0)).last(),
            Some(&ItemState::Failed)
        );
    }

    #[test]
    fn cancelling_an_item_leaves_the_others_running() {
        let processor = Arc::new(Scripted::default());
        let detector = DetectorHandle::spawn(Box::new(NoFaces)).unwrap();
        let handle = start(
            job(&["slow", "ok", "ok"], ReviewMode::Off, 2),
            processor.clone(),
            detector,
        )
        .unwrap();
        while !handle.progress().snapshot().contains_key(&ItemId(0)) {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(handle.cancel_item(ItemId(0)));
        assert!(!handle.cancel_item(ItemId(9)));
        let report = handle.wait();
        assert_eq!(report.items[0].state, ItemState::Cancelled);
        assert_eq!(report.completed(), 2);
    }

    #[test]
    fn cancelling_the_run_cancels_running_and_unstarted_items() {
        let processor = Arc::new(Scripted::default());
        let detector = DetectorHandle::spawn(Box::new(NoFaces)).unwrap();
        let inputs = ["slow"; 10];
        let handle = start(
            job(&inputs, ReviewMode::Off, 2),
            processor.clone(),
            detector,
        )
        .unwrap();
        while handle.progress().snapshot().len() < 2 {
            std::thread::sleep(Duration::from_millis(1));
        }
        handle.cancel();
        let report = handle.wait();
        assert_eq!(report.items.len(), 10);
        assert_eq!(report.count(|s| s == ItemState::Cancelled), 10);
        assert!(processor.published.lock().unwrap().is_empty());
    }

    #[test]
    fn many_items_finish_even_when_events_outnumber_the_channel() {
        let inputs = vec!["ok"; EVENT_CAPACITY * 2];
        let (report, published, events) = run(job(&inputs, ReviewMode::Off, 4));
        assert_eq!(report.completed(), inputs.len());
        assert_eq!(published.len(), inputs.len());
        assert_eq!(events.len(), inputs.len() * 6 + 1);
    }

    #[test]
    fn a_name_conflict_holds_the_item_for_review_in_automatic_mode() {
        let mut job = job(&["ok", "ok"], ReviewMode::Off, 1);
        job.items[1].start = Start::Fresh { needs_review: true };
        let (report, published, _) = run(job);
        assert_eq!(report.items[1].state, ItemState::NeedsReview);
        assert_eq!(published, [(ItemId(0), PublishBasis::AutomaticUnreviewed)]);
    }

    #[test]
    fn reviewed_items_publish_only_after_approval_of_every_problem() {
        let mut resolved = gap();
        resolved.resolution = cloakframe_core::coverage::Resolution::Dismissed;
        let mut job = job(&["ok", "ok", "ok"], ReviewMode::Off, 1);
        let reviewed = |decision, evidence| Start::Reviewed {
            decision,
            analysis: Analysis {
                detections: Vec::new(),
                evidence,
            },
        };
        job.items[0].start = reviewed(Decision::Approve, vec![resolved]);
        job.items[1].start = reviewed(Decision::Approve, vec![gap()]);
        job.items[2].start = reviewed(Decision::ExportWithWarnings, vec![gap()]);
        let (report, published, events) = run(job);
        assert_eq!(
            report.items.iter().map(|o| o.state).collect::<Vec<_>>(),
            [
                ItemState::Published(PublishBasis::Reviewed),
                ItemState::Failed,
                ItemState::Published(PublishBasis::ExportedWithWarnings),
            ]
        );
        assert_eq!(report.completed(), 1);
        assert_eq!(published.len(), 2);
        assert_eq!(states_of(&events, ItemId(0))[0], ItemState::NeedsReview);
    }

    #[test]
    fn an_empty_job_finishes_with_an_empty_report() {
        let (report, published, events) = run(job(&[], ReviewMode::Off, 4));
        assert!(report.items.is_empty() && published.is_empty());
        assert_eq!(events.len(), 1);
    }
}
