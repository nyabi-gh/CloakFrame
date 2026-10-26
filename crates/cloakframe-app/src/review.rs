//! A saved session under review: its items in review order, each one's draft, and the renders
//! the decisions start. The session is held (locked) while it is open, each decision renders
//! on a thread of its own as soon as it is made, and the session is saved after each render.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use cloakframe_core::item::{ItemId, ItemState, ReviewMode};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::events::Event;
use cloakframe_engine::media_processor::MediaProcessor;
use cloakframe_engine::review::{Action, Rejection, ReviewDecisions, apply};
use cloakframe_engine::run::{ItemOutcome, ItemProcessor, Job, JobItem, start_under};
use cloakframe_engine::session::{
    Session, SessionError, SessionId, SessionItem, SessionLock, SessionStore, StoredMedia,
};
use cloakframe_engine::startup;
use eframe::egui;

use crate::draft::Draft;
use crate::queue;
use crate::run::{Failure, Folders, RunOptions, processor};
use crate::settings::Settings;
use crate::tools::Tools;

/// Where an item stands in this review.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Waiting,
    /// Its source changed or is gone since analysis, which no decision gets past.
    Changed,
    Rendering,
    Saved(PathBuf),
    Refused(Rejection),
    /// The render failed or was cancelled; the item stays in the session to decide again.
    Failed(String),
}

enum Rendered {
    Outcome(Box<ItemOutcome>),
    Failed(ItemId, String),
}

pub struct Review {
    pub id: SessionId,
    store: SessionStore,
    /// Taken when the last item is saved, so the session can be removed.
    lock: Option<SessionLock>,
    pub session: Session,
    /// The items in review order; saved ones stay listed, the session no longer holds them.
    pub order: Vec<SessionItem>,
    pub status: BTreeMap<ItemId, Status>,
    /// The action each item was last decided with, to decide again under another name.
    actions: BTreeMap<ItemId, Action>,
    pub drafts: BTreeMap<ItemId, Draft>,
    pub options: RunOptions,
    /// The session does not say what its run processed with (the CLI made it), so the main
    /// screen's settings are used and the engine refuses what they do not match.
    pub options_guessed: bool,
    processor: Arc<MediaProcessor>,
    jobs: mpsc::Sender<JobItem>,
    rendered: mpsc::Receiver<Rendered>,
    cancel: CancelToken,
    pub problems: Vec<String>,
}

#[derive(Debug)]
pub enum OpenError {
    Session(SessionError),
    Processor(Failure),
    Thread(std::io::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(e) => e.fmt(f),
            Self::Processor(Failure::Other(e)) => f.write_str(e),
            Self::Processor(e) => write!(f, "{e:?}"),
            Self::Thread(e) => e.fmt(f),
        }
    }
}

impl Review {
    pub fn open(
        id: SessionId,
        folders: &Folders,
        tools: &Tools,
        settings: &Settings,
        ctx: &egui::Context,
    ) -> Result<Self, OpenError> {
        let store = folders.sessions().ok_or_else(|| {
            OpenError::Session(SessionError::Io(std::io::Error::other(
                "no app data folder",
            )))
        })?;
        let lock = store.lock(&id).map_err(OpenError::Session)?;
        let session = store.load(&id).map_err(OpenError::Session)?;
        let stored = session
            .options
            .clone()
            .and_then(|v| serde_json::from_value::<RunOptions>(v).ok());
        let options = stored.unwrap_or_else(|| RunOptions::of(settings));
        let processor = Arc::new(
            processor(&options, tools, store.records_dir(&id)).map_err(OpenError::Processor)?,
        );
        let (jobs, incoming) = mpsc::channel::<JobItem>();
        let (send, rendered) = mpsc::channel();
        let cancel = CancelToken::new();
        let (token, repaint, worker) = (cancel.clone(), ctx.clone(), Arc::clone(&processor));
        std::thread::Builder::new()
            .name("cloakframe-app-review".into())
            .spawn(move || {
                for job in incoming {
                    let item = job.id;
                    let done = render(job, &worker, &token).map_or_else(
                        |e| Rendered::Failed(item, e),
                        |o| Rendered::Outcome(Box::new(o)),
                    );
                    if send.send(done).is_err() {
                        return;
                    }
                    repaint.request_repaint();
                }
            })
            .map_err(OpenError::Thread)?;
        let order: Vec<SessionItem> = queue::order(&session.items)
            .into_iter()
            .map(|i| session.items[i].clone())
            .collect();
        let status = order
            .iter()
            .map(|i| {
                let status = if i.source_unchanged() {
                    Status::Waiting
                } else {
                    Status::Changed
                };
                (i.item, status)
            })
            .collect();
        let drafts = order.iter().map(|i| (i.item, Draft::new(i))).collect();
        Ok(Self {
            id,
            store,
            lock: Some(lock),
            session,
            order,
            status,
            actions: BTreeMap::new(),
            drafts,
            options,
            options_guessed: stored.is_none(),
            processor,
            jobs,
            rendered,
            cancel,
            problems: Vec::new(),
        })
    }

    /// The session's current record of the item; `None` once it is saved.
    pub fn item(&self, item: ItemId) -> Option<&SessionItem> {
        self.session.items.iter().find(|i| i.item == item)
    }

    pub fn status(&self, item: ItemId) -> &Status {
        self.status.get(&item).unwrap_or(&Status::Waiting)
    }

    pub fn last_action(&self, item: ItemId) -> Option<Action> {
        self.actions.get(&item).copied()
    }

    pub fn is_rendering(&self) -> bool {
        self.status.values().any(|s| *s == Status::Rendering)
    }

    /// Whether the item can be decided now.
    pub fn is_open(&self, item: ItemId) -> bool {
        self.item(item).is_some()
            && matches!(
                self.status(item),
                Status::Waiting | Status::Refused(_) | Status::Failed(_)
            )
    }

    /// Applies the item's draft as `action` and, if the engine accepts it, renders the item.
    pub fn decide(&mut self, item: ItemId, action: Action, rename_to: Option<String>) {
        let (Some(stored), Some(draft)) = (self.item(item), self.drafts.get(&item)) else {
            return;
        };
        let decision = draft.decision(stored, action, rename_to);
        self.actions.insert(item, action);
        let processor = Arc::clone(&self.processor);
        let applied = apply(
            &mut self.session,
            &ReviewDecisions {
                items: vec![decision],
            },
            |input| processor.fingerprint(input),
        );
        if let Some((_, rejection)) = applied.rejected.into_iter().next() {
            self.status.insert(item, Status::Refused(rejection));
            return;
        }
        for job in applied.jobs {
            self.status.insert(job.id, Status::Rendering);
            if self.jobs.send(job).is_err() {
                self.status
                    .insert(item, Status::Failed("the render thread ended".to_owned()));
            }
        }
    }

    /// Takes the renders that ended since the last frame and saves the session after them.
    pub fn take(&mut self) -> bool {
        let mut any = false;
        while let Ok(rendered) = self.rendered.try_recv() {
            self.rendered_now(rendered);
            any = true;
        }
        if any {
            self.save();
        }
        any
    }

    /// Waits for every render in progress, for tests.
    pub fn wait(&mut self) {
        while self.is_rendering() {
            match self.rendered.recv() {
                Ok(rendered) => self.rendered_now(rendered),
                Err(_) => break,
            }
        }
        self.save();
    }

    fn rendered_now(&mut self, rendered: Rendered) {
        match rendered {
            Rendered::Outcome(outcome) => {
                let status = match outcome.state {
                    ItemState::Published(_) => {
                        self.session.items.retain(|i| i.item != outcome.item);
                        Status::Saved(outcome.destination.clone())
                    }
                    ItemState::Cancelled => Status::Failed("cancelled".to_owned()),
                    _ => Status::Failed(
                        outcome
                            .failure_reason()
                            .map(str::to_owned)
                            .unwrap_or_else(|| "the item was not saved".to_owned()),
                    ),
                };
                self.status.insert(outcome.item, status);
            }
            Rendered::Failed(item, e) => {
                self.status.insert(item, Status::Failed(e));
            }
        }
    }

    /// Stores what the session still holds; once it holds nothing, removes it.
    fn save(&mut self) {
        let result = if self.session.items.is_empty() {
            // Windows does not remove a lock file that is open.
            self.lock = None;
            self.store.remove(&self.id)
        } else {
            self.store
                .save(&self.id, &self.session)
                .and_then(|()| self.store.prune(&self.id, &self.session))
        };
        if let Err(e) = result {
            self.problems
                .push(format!("cannot save the review session: {e}"));
        }
    }

    /// Stops the renders in progress; they end as failed and the items stay in the session.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_done(&self) -> bool {
        self.session.items.is_empty()
    }

    pub fn position(&self, item: ItemId) -> Option<usize> {
        self.order.iter().position(|i| i.item == item)
    }

    /// The next item after `item`, in review order, that can still be decided.
    pub fn next_open(&self, item: ItemId) -> Option<ItemId> {
        let at = self.position(item)?;
        self.order[at + 1..]
            .iter()
            .chain(&self.order[..at])
            .map(|i| i.item)
            .find(|&i| self.is_open(i))
    }

    /// The processor the session's items render with, which also opens a video's preview.
    pub fn processor(&self) -> Arc<MediaProcessor> {
        Arc::clone(&self.processor)
    }

    pub fn is_video(&self, item: ItemId) -> bool {
        self.order
            .iter()
            .any(|i| i.item == item && matches!(i.media, StoredMedia::Video { .. }))
    }
}

impl Drop for Review {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Renders one decided item as a run of its own. Reviewed items are not analyzed again, so no
/// model is loaded.
fn render(
    job: JobItem,
    processor: &Arc<MediaProcessor>,
    cancel: &CancelToken,
) -> Result<ItemOutcome, String> {
    let started = startup::without_models().map_err(|e| e.to_string())?;
    let handle = start_under(
        Job {
            items: vec![job],
            review: ReviewMode::Required,
            workers: 1,
        },
        Arc::clone(processor),
        started.handle,
        cancel,
    )
    .map_err(|e| e.to_string())?;
    let mut report = None;
    for event in handle.events().iter() {
        if let Event::RunFinished(r) = event {
            report = Some(r);
        }
    }
    handle
        .finish()
        .or(report)
        .and_then(|r| r.items.into_iter().next())
        .ok_or_else(|| "the render ended without a report".to_owned())
}
