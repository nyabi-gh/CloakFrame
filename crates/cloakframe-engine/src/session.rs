use std::fmt;
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, RandomState};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cloakframe_core::coverage::CoverageEvidence;
use cloakframe_core::detection::Detection;
use cloakframe_core::item::ItemId;
use cloakframe_core::review::ReviewRevision;
use serde::{Deserialize, Serialize};

use crate::identity::SourceStamp;
use crate::output_plan::Conflict;
use crate::publish::TEMP_PREFIX;

pub const FORMAT_VERSION: u32 = 1;

/// Results waiting for review: detections and review decisions only, never pixels (plan 4.2).
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
    pub detections: Vec<Detection>,
    pub evidence: Vec<CoverageEvidence>,
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
}

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    Corrupt(serde_json::Error),
    UnsupportedVersion(u32),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "session could not be read: {e}"),
            Self::Corrupt(e) => write!(f, "session is damaged: {e}"),
            Self::UnsupportedVersion(v) => write!(f, "session format {v} is not supported"),
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

    pub fn remove(&self, id: &SessionId) -> io::Result<()> {
        match fs::remove_file(self.path(id)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
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
        fs::write(store.path(&id), b"{\"version\": 1, \"items\": [").unwrap();
        assert!(matches!(store.load(&id), Err(SessionError::Corrupt(_))));
        fs::write(store.path(&id), b"{\"version\": 2}").unwrap();
        assert!(matches!(
            store.load(&id),
            Err(SessionError::UnsupportedVersion(2))
        ));
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
        let id = SessionId::generate();
        assert_eq!(SessionId::parse(id.as_str()), Some(id));
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
