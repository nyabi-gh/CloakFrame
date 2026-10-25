use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::hash::{BuildHasher, RandomState};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cloakframe_core::coverage::CoverageEvidence;
use cloakframe_core::detection::Detection;
use cloakframe_core::item::ItemId;
use cloakframe_core::review::{PhotoEdits, ReviewRevision, VideoEdits};
use serde::{Deserialize, Serialize};

use crate::identity::SourceStamp;
use crate::output_plan::Conflict;
use crate::publish::TEMP_PREFIX;
use crate::video::tracking::TrackSummary;

/// 2 holds videos (M5). No build that wrote 1 was released, so 1 is not read.
pub const FORMAT_VERSION: u32 = 2;

/// Results waiting for review: detections and review decisions only, never pixels (plan 4.2). A
/// video's frame-by-frame detections and regions are in its records folder beside the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub output: PathBuf,
    pub items: Vec<SessionItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionItem {
    pub item: ItemId,
    pub input: PathBuf,
    /// Relative to the session's output folder.
    pub destination: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<Conflict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<PathBuf>,
    pub source: SourceStamp,
    pub revision: ReviewRevision,
    #[serde(flatten)]
    pub media: StoredMedia,
    pub evidence: Vec<CoverageEvidence>,
}

/// What an item's review works on, by the kind of item. Edits are the latest accepted review
/// decision's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "media", rename_all = "snake_case")]
pub enum StoredMedia {
    Photo {
        detections: Vec<Detection>,
        #[serde(default, skip_serializing_if = "PhotoEdits::is_empty")]
        edits: PhotoEdits,
    },
    Video {
        /// Source frames, which the records, gaps and edits count.
        frames: u64,
        /// The display size the regions are in.
        width: u32,
        height: u32,
        /// The source's length, [`crate::video::process::VideoAnalysis::seconds`].
        seconds: f64,
        /// Every kept track with its kind (P-2); the boxes are in the records.
        tracks: Vec<TrackSummary>,
        #[serde(default, skip_serializing_if = "VideoEdits::is_empty")]
        edits: VideoEdits,
    },
}

impl SessionItem {
    /// A changed or missing source invalidates the stored results.
    pub fn source_unchanged(&self) -> bool {
        SourceStamp::of(&self.input).is_ok_and(|now| now == self.source)
    }
}

/// File name of a stored session. Generated, never taken from user input, so it cannot name a
/// path outside the store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(String);

impl SessionId {
    pub fn generate() -> Self {
        let secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let random = RandomState::new().hash_one(SystemTime::now());
        Self(format!("{secs:012x}-{random:016x}"))
    }

    pub fn parse(s: &str) -> Option<Self> {
        let valid = !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| (b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) || b == b'-');
        valid.then(|| Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// When [`Self::generate`] made it; `None` for an id it did not make.
    pub fn created(&self) -> Option<SystemTime> {
        let (secs, _) = self.0.split_once('-')?;
        let secs = u64::from_str_radix(secs, 16).ok()?;
        SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs))
    }
}

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    Corrupt(serde_json::Error),
    UnsupportedVersion(u32),
    /// Another run or review holds the session.
    InUse,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "session could not be read: {e}"),
            Self::Corrupt(e) => write!(f, "session is damaged: {e}"),
            Self::UnsupportedVersion(v) => write!(f, "session format {v} is not supported"),
            Self::InUse => f.write_str("the session is in use by another run or review"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, id: &SessionId) -> PathBuf {
        self.dir.join(format!("{}.json", id.0))
    }

    /// Replaces the stored session atomically: a crash leaves either the old or the new one.
    pub fn save(&self, id: &SessionId, session: &Session) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let temp = self.dir.join(format!("{TEMP_PREFIX}{}.json", id.0));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp)?;
            serde_json::to_writer(&mut file, session)?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, self.path(id))?;
            #[cfg(unix)]
            fs::File::open(&self.dir)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    pub fn load(&self, id: &SessionId) -> Result<Session, SessionError> {
        let bytes = fs::read(self.path(id))?;
        #[derive(Deserialize)]
        struct Version {
            version: u32,
        }
        let version: Version = serde_json::from_slice(&bytes).map_err(SessionError::Corrupt)?;
        if version.version != FORMAT_VERSION {
            return Err(SessionError::UnsupportedVersion(version.version));
        }
        serde_json::from_slice(&bytes).map_err(SessionError::Corrupt)
    }

    /// Stored sessions, oldest first. Temp files and anything else in the folder are ignored.
    pub fn list(&self) -> io::Result<Vec<SessionId>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let name = entry?.file_name();
            if let Some(id) = name
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
                .and_then(SessionId::parse)
            {
                ids.push(id);
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Removes the session with its records and lock.
    pub fn remove(&self, id: &SessionId) -> io::Result<()> {
        let gone = |r: io::Result<()>| match r {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
        gone(fs::remove_file(self.path(id)))?;
        gone(fs::remove_dir_all(self.records_dir(id)))?;
        gone(fs::remove_file(self.lock_path(id)))
    }

    fn lock_path(&self, id: &SessionId) -> PathBuf {
        self.dir.join(format!("{}.lock", id.0))
    }

    /// Holds the session for this process until the lock is dropped: [`sweep`](Self::sweep)
    /// leaves its records alone, and a second run or review of it is refused. The system releases
    /// the lock however the process ends, so a session whose run crashed can be cleaned up.
    pub fn lock(&self, id: &SessionId) -> Result<SessionLock, SessionError> {
        fs::create_dir_all(&self.dir)?;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.lock_path(id))?;
        match file.try_lock() {
            Ok(()) => Ok(SessionLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(SessionError::InUse),
            Err(TryLockError::Error(e)) => Err(e.into()),
        }
    }

    /// Removes the records folders of `session`'s items that are not videos it still holds.
    pub fn prune(&self, id: &SessionId, session: &Session) -> io::Result<()> {
        let kept: Vec<String> = session
            .items
            .iter()
            .filter(|i| matches!(i.media, StoredMedia::Video { .. }))
            .map(|i| crate::video::process::item_records_dir(Path::new(""), i.item))
            .filter_map(|p| p.to_str().map(str::to_owned))
            .collect();
        let entries = match fs::read_dir(self.records_dir(id)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("item-") && !kept.iter().any(|k| *k == name) {
                fs::remove_dir_all(entry.path())?;
            }
        }
        if kept.is_empty() {
            match fs::remove_dir(self.records_dir(id)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }

    /// Removes the records no stored session needs: those of a run that ended without saving
    /// its session (a crash or a kill) and items a session no longer holds. Sessions another
    /// process holds, and records with no lock beside them, are left alone; an unreadable
    /// session keeps its records. Returns what could not be removed; fails if the store cannot
    /// be read.
    pub fn sweep(&self) -> io::Result<Vec<(PathBuf, io::Error)>> {
        let mut failed = Vec::new();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(failed),
            Err(e) => return Err(e),
        };
        let ids: Vec<SessionId> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .and_then(|n| n.strip_suffix(".lock"))
                    .and_then(SessionId::parse)
            })
            .collect();
        for id in ids {
            let Ok(file) = File::options().write(true).open(self.lock_path(&id)) else {
                continue;
            };
            if file.try_lock().is_err() {
                continue;
            }
            let result = match self.load(&id) {
                Ok(session) => self.prune(&id, &session),
                Err(SessionError::Io(e)) if e.kind() == io::ErrorKind::NotFound => self.remove(&id),
                Err(_) => Ok(()),
            };
            if let Err(e) = result {
                failed.push((self.records_dir(&id), e));
            }
        }
        Ok(failed)
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The folder for the analysis records of the session's videos, beside its file.
    pub fn records_dir(&self, id: &SessionId) -> PathBuf {
        self.dir.join(&id.0)
    }
}

/// See [`SessionStore::lock`].
#[derive(Debug)]
pub struct SessionLock {
    _file: File,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use cloakframe_core::coverage::Problem;
    use cloakframe_core::detection::ModelId;
    use cloakframe_core::geometry::Rect;
    use cloakframe_core::review::{Digest, ReviewKey};

    fn session(dir: &TestDir) -> Session {
        let input = dir.write("in/photo.jpg");
        let key = ReviewKey {
            input: Digest([1; 32]),
            models: Digest([2; 32]),
            settings: Digest([3; 32]),
            timeline: Digest([4; 32]),
        };
        Session {
            version: FORMAT_VERSION,
            output: dir.path().join("out"),
            items: vec![SessionItem {
                item: ItemId(0),
                destination: PathBuf::from("photo.jpg"),
                conflict: None,
                suggestion: None,
                source: SourceStamp::of(&input).unwrap(),
                input,
                revision: ReviewRevision::first(key),
                media: StoredMedia::Photo {
                    detections: vec![
                        Detection::new(
                            ModelId::YuNet,
                            0.9,
                            Rect::new(1.0, 2.0, 30.0, 40.0).unwrap(),
                            None,
                            None,
                        )
                        .unwrap(),
                    ],
                    edits: PhotoEdits {
                        excluded: Vec::new(),
                        added: vec![Rect::new(50.0, 60.0, 10.0, 10.0).unwrap()],
                    },
                },
                evidence: vec![CoverageEvidence::unresolved(Problem::CappedCandidates {
                    model: ModelId::YuNet,
                    frame: None,
                    kept: 100,
                    found: 259,
                })],
            }],
        }
    }

    #[test]
    fn saves_and_loads_a_session() {
        let dir = TestDir::new();
        let store = SessionStore::new(dir.path().join("sessions"));
        let id = SessionId::generate();
        let stored = session(&dir);
        store.save(&id, &stored).unwrap();
        assert_eq!(store.load(&id).unwrap(), stored);
        assert_eq!(store.list().unwrap(), std::slice::from_ref(&id));
        assert!(stored.items[0].source_unchanged());

        let mut updated = stored.clone();
        updated.items[0].evidence.clear();
        store.save(&id, &updated).unwrap();
        assert_eq!(store.load(&id).unwrap(), updated);
        assert_eq!(fs::read_dir(store.dir()).unwrap().count(), 1);

        store.remove(&id).unwrap();
        store.remove(&id).unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn a_changed_source_invalidates_the_item() {
        let dir = TestDir::new();
        let stored = session(&dir);
        fs::write(&stored.items[0].input, b"edited").unwrap();
        assert!(!stored.items[0].source_unchanged());
        fs::remove_file(&stored.items[0].input).unwrap();
        assert!(!stored.items[0].source_unchanged());
    }

    #[test]
    fn rejects_damaged_and_future_sessions() {
        let dir = TestDir::new();
        let store = SessionStore::new(dir.path());
        let id = SessionId::generate();
        fs::write(store.path(&id), b"{\"version\": 2, \"items\": [").unwrap();
        assert!(matches!(store.load(&id), Err(SessionError::Corrupt(_))));
        for version in [1, 3] {
            fs::write(store.path(&id), format!("{{\"version\": {version}}}")).unwrap();
            assert!(matches!(
                store.load(&id),
                Err(SessionError::UnsupportedVersion(v)) if v == version
            ));
        }
        assert!(matches!(
            store.load(&SessionId::generate()),
            Err(SessionError::Io(_))
        ));
    }

    #[test]
    fn stores_no_pixels() {
        let dir = TestDir::new();
        let store = SessionStore::new(dir.path().join("sessions"));
        let id = SessionId::generate();
        store.save(&id, &session(&dir)).unwrap();
        assert!(fs::metadata(store.path(&id)).unwrap().len() < 2048);
    }

    #[test]
    fn session_ids_cannot_name_other_paths() {
        assert!(SessionId::parse("../etc/passwd").is_none());
        assert!(SessionId::parse("a/b").is_none());
        assert!(SessionId::parse("").is_none());
        assert!(SessionId::parse("ABC").is_none());
        let before = SystemTime::now() - Duration::from_secs(1);
        let id = SessionId::generate();
        assert_eq!(SessionId::parse(id.as_str()).as_ref(), Some(&id));
        let created = id.created().unwrap();
        assert!(
            created >= before && created <= SystemTime::now(),
            "{created:?}"
        );
        assert_eq!(SessionId::parse("abc").unwrap().created(), None);
    }

    fn video(dir: &TestDir, item: u64) -> SessionItem {
        let mut photo = session(dir).items.remove(0);
        photo.item = ItemId(item);
        SessionItem {
            destination: PathBuf::from(format!("clip-{item}.mp4")),
            media: StoredMedia::Video {
                frames: 90,
                width: 640,
                height: 360,
                seconds: 3.0,
                tracks: vec![TrackSummary {
                    id: 1,
                    kind: cloakframe_core::detection::DetectionKind::Face,
                    first: 0,
                    last: 89,
                    detections: 55,
                    strong: 50,
                    low_confidence: false,
                }],
                edits: VideoEdits::default(),
            },
            evidence: vec![CoverageEvidence::unresolved(Problem::UnresolvedGap {
                detection: cloakframe_core::detection::DetectionKind::Face,
                tracks: vec![1],
                first: 30,
                last: 64,
            })],
            ..photo
        }
    }

    /// P-2: a stored video's tracks and gaps each say what they follow.
    #[test]
    fn a_video_is_stored_with_the_kinds_of_its_tracks_and_gaps() {
        let dir = TestDir::new();
        let store = SessionStore::new(dir.path().join("sessions"));
        let id = SessionId::generate();
        let mut stored = session(&dir);
        stored.items.push(video(&dir, 1));
        store.save(&id, &stored).unwrap();
        assert_eq!(store.load(&id).unwrap(), stored);
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path(&id)).unwrap()).unwrap();
        let item = &json["items"][1];
        assert_eq!(item["media"], "video");
        assert_eq!(item["tracks"][0]["kind"], "face");
        assert_eq!(item["evidence"][0]["kind"], "unresolved_gap");
        assert_eq!(item["evidence"][0]["detection"], "face");
        assert_eq!(json["items"][0]["media"], "photo");
        assert_eq!(json["items"][0]["detections"][0]["kind"], "face");
    }

    #[test]
    fn a_session_is_held_by_one_process_at_a_time() {
        let dir = TestDir::new();
        let store = SessionStore::new(dir.path().join("sessions"));
        let id = SessionId::generate();
        let lock = store.lock(&id).unwrap();
        assert!(matches!(store.lock(&id), Err(SessionError::InUse)));
        drop(lock);
        store.lock(&id).unwrap();
    }

    /// What a crashed run left goes; what a held session, an unreadable one or one still holding
    /// the item needs stays.
    #[test]
    fn the_sweep_removes_only_records_no_session_needs() {
        let dir = TestDir::new();
        let store = SessionStore::new(dir.path().join("sessions"));
        let item_dir = |id: &SessionId, item: u64| {
            let path = store.records_dir(id).join(format!("item-{item}"));
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("regions"), b"x").unwrap();
            path
        };

        let crashed = SessionId::generate();
        drop(store.lock(&crashed).unwrap());
        item_dir(&crashed, 0);

        let running = SessionId::generate();
        let held = store.lock(&running).unwrap();
        let running_records = item_dir(&running, 0);

        let stored = SessionId::generate();
        drop(store.lock(&stored).unwrap());
        let mut session = session(&dir);
        session.items.push(video(&dir, 3));
        store.save(&stored, &session).unwrap();
        let published = item_dir(&stored, 2);
        let waiting = item_dir(&stored, 3);
        let photo = item_dir(&stored, 0);

        let unreadable = SessionId::generate();
        drop(store.lock(&unreadable).unwrap());
        fs::write(store.path(&unreadable), b"{").unwrap();
        let unreadable_records = item_dir(&unreadable, 0);

        let unlocked = SessionId::generate();
        let unlocked_records = item_dir(&unlocked, 0);

        assert!(store.sweep().unwrap().is_empty());
        assert!(!store.records_dir(&crashed).exists());
        assert!(!store.lock_path(&crashed).exists());
        assert!(running_records.exists());
        assert!(!published.exists() && !photo.exists());
        assert!(waiting.exists());
        assert!(unreadable_records.exists());
        assert!(unlocked_records.exists());
        drop(held);

        session.items.pop();
        store.prune(&stored, &session).unwrap();
        assert!(!store.records_dir(&stored).exists());
        store.remove(&running).unwrap();
        assert!(!store.records_dir(&running).exists() && !store.lock_path(&running).exists());
    }

    #[test]
    fn listing_ignores_other_files_and_a_missing_folder() {
        let dir = TestDir::new();
        let store = SessionStore::new(dir.path().join("sessions"));
        assert!(store.list().unwrap().is_empty());
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(store.dir().join("notes.txt"), b"x").unwrap();
        fs::write(store.dir().join(".cloakframe-tmp-0.json"), b"x").unwrap();
        assert!(store.list().unwrap().is_empty());
    }
}
