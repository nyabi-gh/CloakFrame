//! The review sessions earlier runs left, which the main screen lists so they can be opened
//! again after the app restarts (plan 6, M6 scope ②).

use std::io;
use std::sync::mpsc;
use std::time::SystemTime;

use cloakframe_engine::session::{Session, SessionError, SessionId, SessionStore};
use eframe::egui;

use crate::run::Folders;

/// A stored session as the list shows it.
#[derive(Debug, Clone)]
pub struct Saved {
    pub id: SessionId,
    pub created: Option<SystemTime>,
    pub content: Result<Loaded, String>,
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub session: Session,
    /// For each item, whether its source changed or went since it was analyzed, which a review
    /// cannot get past.
    pub changed: Vec<bool>,
}

impl Loaded {
    pub fn changed_count(&self) -> usize {
        self.changed.iter().filter(|&&c| c).count()
    }
}

fn load(folders: &Folders) -> io::Result<Vec<Saved>> {
    let Some(store) = folders.sessions() else {
        return Ok(Vec::new());
    };
    let mut saved: Vec<Saved> = store
        .list()?
        .into_iter()
        .map(|id| {
            let content = store
                .load(&id)
                .map(|session| Loaded {
                    changed: session
                        .items
                        .iter()
                        .map(|i| !i.source_unchanged())
                        .collect(),
                    session,
                })
                .map_err(|e| e.to_string());
            Saved {
                created: id.created(),
                id,
                content,
            }
        })
        .collect();
    saved.sort_by_key(|s| std::cmp::Reverse(s.created));
    Ok(saved)
}

/// The list, read on a thread of its own: reading a session stats each of its sources.
pub struct SavedSessions {
    folders: Folders,
    loading: Option<mpsc::Receiver<io::Result<Vec<Saved>>>>,
    pub list: Vec<Saved>,
    pub error: Option<String>,
}

impl SavedSessions {
    pub fn new(folders: Folders) -> Self {
        Self {
            folders,
            loading: None,
            list: Vec::new(),
            error: None,
        }
    }

    pub fn reload(&mut self, ctx: &egui::Context) {
        let (send, loading) = mpsc::channel();
        let (folders, repaint) = (self.folders.clone(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("cloakframe-sessions".into())
            .spawn(move || {
                let _ = send.send(load(&folders));
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => self.loading = Some(loading),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn take(&mut self) {
        if let Some(loading) = &self.loading
            && let Ok(result) = loading.try_recv()
        {
            self.loaded(result);
        }
    }

    /// Waits for the list being read, for tests.
    pub fn wait(&mut self) {
        if let Some(loading) = &self.loading
            && let Ok(result) = loading.recv()
        {
            self.loaded(result);
        }
    }

    fn loaded(&mut self, result: io::Result<Vec<Saved>>) {
        self.loading = None;
        match result {
            Ok(list) => {
                self.list = list;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Removes the session and its video records; the sources and the saved outputs stay. The
    /// caller reads the list again for anything else that changed meanwhile.
    pub fn discard(&mut self, id: &SessionId) -> Result<(), SessionError> {
        let store: SessionStore = self
            .folders
            .sessions()
            .ok_or_else(|| SessionError::Io(io::Error::other("no app data folder")))?;
        // Refused while a run or a review holds it. The lock goes before the files: Windows does
        // not remove a lock file that is open.
        drop(store.lock(id)?);
        store.remove(id)?;
        self.list.retain(|s| &s.id != id);
        // A read still in flight may have listed the session before it was removed.
        self.loading = None;
        Ok(())
    }
}

/// Whole days from `created` to `now`, for "today", "yesterday" and "N days ago".
pub fn age_days(created: SystemTime, now: SystemTime) -> u64 {
    now.duration_since(created)
        .map_or(0, |d| d.as_secs() / (24 * 60 * 60))
}
