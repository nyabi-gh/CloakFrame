//! Updating the installed app (plan 10): checking the feed, asking the user, downloading and
//! restarting into the new version. Velopack does the work on Windows and Linux; an update
//! installs only when the key this build pins signed it (P-7). Each step runs on its own thread.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, mpsc};

use cloakframe_engine::update::Refusal;
use eframe::egui;
use fluent_bundle::FluentValue;

use crate::i18n::Translator;
use crate::style::{self, Palette};

/// The public half of the update key, which `build.rs` requires of release builds.
pub const PINNED_KEY: Option<&str> = option_env!("CLOAKFRAME2_UPDATE_PUBLIC_KEY");

/// A newer version the feed offers, whose signature has been checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    pub channel: String,
    pub package_id: String,
    pub file_name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The network, the disk or the updater failed; nothing was installed.
    Failed(String),
    /// The update did not prove it came from the key this build pins.
    Refused(Refusal),
}

impl From<Refusal> for Problem {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

/// Velopack, or a stand-in for tests. Each call blocks.
pub trait Backend: Send + Sync {
    /// A newer, signed version, or None when this one is the newest.
    fn check(&self) -> Result<Option<Offer>, Problem>;
    /// Downloads the package and checks it is the one signed; `progress` gets 0 to 100.
    fn download(&self, offer: &Offer, progress: &(dyn Fn(u8) + Sync)) -> Result<(), Problem>;
    /// Starts the updater, which installs the package once the app has quit and starts it again.
    fn apply(&self, offer: &Offer) -> Result<(), Problem>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Checking {
        asked: bool,
    },
    /// Shown only when the user asked.
    UpToDate,
    CheckFailed(String),
    Available(Offer),
    Downloading(Offer),
    Ready(Offer),
    Applying(Offer),
    /// The updater waits for the app to quit.
    Quitting,
    Failed(String),
    Refused(String),
}

enum Done {
    Checked {
        asked: bool,
        result: Result<Option<Offer>, Problem>,
    },
    Downloaded(Offer, Result<(), Problem>),
    Applied(Result<(), Problem>),
}

pub struct Updates {
    backend: Result<Arc<dyn Backend>, String>,
    pub stage: Stage,
    job: Option<mpsc::Receiver<Done>>,
    percent: Arc<AtomicU8>,
}

impl Updates {
    /// No updates, with the reason a check the user asks for reports.
    pub fn unavailable(reason: String) -> Self {
        Self {
            backend: Err(reason),
            stage: Stage::Idle,
            job: None,
            percent: Arc::new(AtomicU8::new(0)),
        }
    }

    pub fn with(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend: Ok(backend),
            ..Self::unavailable(String::new())
        }
    }

    /// The installed app's updater; where the app is not installed by Velopack, or the build pins
    /// no key, the reason there is none.
    pub fn installed() -> Self {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            if PINNED_KEY.is_none() {
                return Self::unavailable(Refusal::NoKey.to_string());
            }
            match velopack_backend::Velopack::new() {
                Ok(backend) => Self::with(Arc::new(backend)),
                Err(reason) => Self::unavailable(reason),
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        Self::unavailable("this build does not update itself".into())
    }

    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// `asked` is the user asking from the menu; a check at startup says nothing unless there is
    /// an update or one was refused.
    pub fn check(&mut self, asked: bool, ctx: &egui::Context) {
        if self.is_busy() || matches!(self.stage, Stage::Ready(_) | Stage::Quitting) {
            return;
        }
        let backend = match &self.backend {
            Ok(backend) => backend.clone(),
            Err(reason) => {
                if asked {
                    self.stage = Stage::CheckFailed(reason.clone());
                }
                return;
            }
        };
        self.stage = Stage::Checking { asked };
        self.spawn(ctx, move |_| Done::Checked {
            asked,
            result: backend.check(),
        });
    }

    /// Turning the check at startup off drops the answer to one still under way (plan 10).
    pub fn forget_startup_check(&mut self) {
        if self.stage == (Stage::Checking { asked: false }) {
            self.job = None;
            self.stage = Stage::Idle;
        }
    }

    fn download(&mut self, offer: Offer, ctx: &egui::Context) {
        let Ok(backend) = self.backend.clone() else {
            return;
        };
        self.percent.store(0, Ordering::Relaxed);
        self.stage = Stage::Downloading(offer.clone());
        let percent = self.percent.clone();
        self.spawn(ctx, move |repaint| {
            let result = backend.download(&offer, &|p| {
                percent.store(p.min(100), Ordering::Relaxed);
                repaint.request_repaint();
            });
            Done::Downloaded(offer, result)
        });
    }

    fn apply(&mut self, offer: Offer, ctx: &egui::Context) {
        let Ok(backend) = self.backend.clone() else {
            return;
        };
        self.stage = Stage::Applying(offer.clone());
        self.spawn(ctx, move |_| {
            let result = backend.apply(&offer);
            Done::Applied(result)
        });
    }

    fn spawn(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(&egui::Context) -> Done + Send + 'static,
    ) {
        let (send, done) = mpsc::channel();
        let repaint = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("cloakframe-update".into())
            .spawn(move || {
                let _ = send.send(work(&repaint));
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => self.job = Some(done),
            Err(e) => self.stage = Stage::Failed(e.to_string()),
        }
    }

    /// Takes what the thread finished since the last frame; true when the app should quit so the
    /// updater can install.
    pub fn take(&mut self) -> bool {
        let Some(done) = self.job.as_ref().and_then(|j| j.try_recv().ok()) else {
            return false;
        };
        self.job = None;
        self.finish(done)
    }

    /// Waits for the thread at work, for tests.
    pub fn wait(&mut self) -> bool {
        match self.job.take().and_then(|j| j.recv().ok()) {
            Some(done) => self.finish(done),
            None => false,
        }
    }

    fn finish(&mut self, done: Done) -> bool {
        self.stage = match done {
            Done::Checked { asked, result } => match result {
                Ok(Some(offer)) => Stage::Available(offer),
                Ok(None) if asked => Stage::UpToDate,
                Ok(None) => Stage::Idle,
                Err(Problem::Refused(r)) => Stage::Refused(r.to_string()),
                Err(Problem::Failed(e)) if asked => Stage::CheckFailed(e),
                Err(Problem::Failed(e)) => {
                    eprintln!("could not check for updates: {e}");
                    Stage::Idle
                }
            },
            Done::Downloaded(offer, result) => match result {
                Ok(()) => Stage::Ready(offer),
                Err(Problem::Refused(r)) => Stage::Refused(r.to_string()),
                Err(Problem::Failed(e)) => Stage::Failed(e),
            },
            Done::Applied(result) => match result {
                Ok(()) => Stage::Quitting,
                Err(Problem::Refused(r)) => Stage::Refused(r.to_string()),
                Err(Problem::Failed(e)) => Stage::Failed(e),
            },
        };
        self.stage == Stage::Quitting
    }

    /// The update's line above the files, if there is anything to say. `working` holds the restart
    /// back while a run or a review's renders work.
    pub fn bar(&mut self, ui: &mut egui::Ui, tr: &Translator, working: bool) {
        let p = *Palette::of(ui);
        let version = |offer: &Offer| [("version", FluentValue::from(offer.version.clone()))];
        let ctx = ui.ctx().clone();
        let mut next = None;
        match &self.stage {
            Stage::Idle | Stage::Checking { asked: false } | Stage::Quitting => return,
            Stage::Checking { asked: true } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr.get("update-checking"));
                });
            }
            Stage::UpToDate => {
                ui.horizontal(|ui| {
                    ui.label(tr.with(
                        "update-none",
                        &[("version", FluentValue::from(env!("CARGO_PKG_VERSION")))],
                    ));
                    if style::plain(ui, tr.get("close")).clicked() {
                        next = Some(Stage::Idle);
                    }
                });
            }
            Stage::CheckFailed(error) => {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        p.failed,
                        tr.with(
                            "update-check-failed",
                            &[("error", FluentValue::from(error.as_str()))],
                        ),
                    );
                    if style::plain(ui, tr.get("close")).clicked() {
                        next = Some(Stage::Idle);
                    }
                });
            }
            Stage::Available(offer) => {
                let mut install = false;
                ui.horizontal(|ui| {
                    ui.label(tr.with("update-available", &version(offer)));
                    if style::plain(ui, tr.get("update-install")).clicked() {
                        install = true;
                    }
                    if style::plain(ui, tr.get("update-later")).clicked() {
                        next = Some(Stage::Idle);
                    }
                });
                if install {
                    self.download(offer.clone(), &ctx);
                }
            }
            Stage::Downloading(offer) => {
                ui.horizontal(|ui| {
                    ui.label(tr.with("update-downloading", &version(offer)));
                    let percent = self.percent.load(Ordering::Relaxed);
                    ui.add(egui::ProgressBar::new(f32::from(percent) / 100.0).desired_width(160.0));
                });
            }
            Stage::Ready(offer) | Stage::Applying(offer) => {
                let applying = matches!(self.stage, Stage::Applying(_));
                let mut restart = false;
                ui.horizontal(|ui| {
                    ui.label(tr.with("update-ready", &version(offer)));
                    let button = egui::Button::new(tr.get("update-restart"))
                        .min_size(egui::vec2(0.0, style::BUTTON_HEIGHT))
                        .corner_radius(style::BUTTON_RADIUS);
                    let response = ui.add_enabled(!working && !applying, button);
                    if working {
                        response
                            .clone()
                            .on_disabled_hover_text(tr.get("update-restart-after-run"));
                    }
                    if response.clicked() {
                        restart = true;
                    }
                    if !applying && style::plain(ui, tr.get("update-later")).clicked() {
                        next = Some(Stage::Idle);
                    }
                });
                if restart {
                    self.apply(offer.clone(), &ctx);
                }
            }
            Stage::Failed(error) => {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        p.failed,
                        tr.with(
                            "update-failed",
                            &[("error", FluentValue::from(error.as_str()))],
                        ),
                    );
                    if style::plain(ui, tr.get("close")).clicked() {
                        next = Some(Stage::Idle);
                    }
                });
            }
            Stage::Refused(reason) => {
                ui.horizontal(|ui| {
                    ui.colored_label(p.failed, tr.get("update-refused"))
                        .on_hover_text(reason);
                    if style::plain(ui, tr.get("close")).clicked() {
                        next = Some(Stage::Idle);
                    }
                });
            }
        }
        if let Some(stage) = next {
            self.stage = stage;
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod velopack_backend {
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex};

    use cloakframe_engine::model_files::download::{Fetch, FetchError, Https};
    use cloakframe_engine::update::{self, PackageError, Refusal, Release};
    use velopack::bundle::Manifest;
    use velopack::locator::{self, LocationContext};
    use velopack::sources::UpdateSource;
    use velopack::{
        Error, UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions, VelopackAsset,
        VelopackAssetFeed,
    };

    use super::{Backend, Offer, PINNED_KEY, Problem};

    /// Overrides [`update::FEED`] with a local folder holding a feed, its packages and their
    /// signatures, to test updates before a release publishes them. The signature check is the
    /// same, so a folder can only offer what the pinned key signed.
    const FEED_FOLDER: &str = "CLOAKFRAME_UPDATE_FEED";
    const MAX_FEED: u64 = 4 << 20;
    const MAX_SIGNATURE: u64 = 1024;

    enum Location {
        Web(Https),
        Folder(PathBuf),
    }

    /// The feed, read through the app's own HTTPS client, which sends nothing but the request
    /// for the file: Velopack's own adds the installed version and an ID to every feed request.
    struct Feed {
        location: Location,
        /// The channel Velopack asked for, which the signature covers.
        channel: Mutex<Option<String>>,
    }

    /// Names as `vpk` gives them, so a feed cannot point outside the feed or the packages folder.
    fn plain_name(name: &str) -> Result<&str, Error> {
        let plain = !name.is_empty()
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
        if plain {
            Ok(name)
        } else {
            Err(Error::Other(format!(
                "the feed names '{name}', which is not a file name"
            )))
        }
    }

    impl Feed {
        fn open(&self, name: &str) -> Result<(Option<u64>, Box<dyn Read + Send>), OpenError> {
            let name = plain_name(name).map_err(|e| OpenError::Other(e.to_string()))?;
            match &self.location {
                Location::Web(https) => {
                    let body =
                        https
                            .get(&format!("{}{name}", update::FEED))
                            .map_err(|e| match e {
                                FetchError::Status(404) => OpenError::Missing,
                                e => OpenError::Other(e.to_string()),
                            })?;
                    Ok((body.length, body.reader))
                }
                Location::Folder(folder) => {
                    let file = File::open(folder.join(name)).map_err(|e| match e.kind() {
                        io::ErrorKind::NotFound => OpenError::Missing,
                        _ => OpenError::Other(e.to_string()),
                    })?;
                    Ok((file.metadata().ok().map(|m| m.len()), Box::new(file)))
                }
            }
        }

        fn read(&self, name: &str, limit: u64) -> Result<Vec<u8>, OpenError> {
            let (_, reader) = self.open(name)?;
            let mut bytes = Vec::new();
            reader
                .take(limit + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| OpenError::Other(e.to_string()))?;
            if bytes.len() as u64 > limit {
                return Err(OpenError::Other(format!(
                    "{name} is larger than {limit} bytes"
                )));
            }
            Ok(bytes)
        }
    }

    enum OpenError {
        Missing,
        Other(String),
    }

    impl OpenError {
        fn into_velopack(self, name: &str) -> Error {
            match self {
                Self::Missing => Error::Other(format!("the feed has no {name}")),
                Self::Other(e) => Error::Other(e),
            }
        }
    }

    struct Source(Arc<Feed>);

    impl UpdateSource for Source {
        fn get_release_feed(
            &self,
            channel: &str,
            _: &Manifest,
            _: &str,
        ) -> Result<VelopackAssetFeed, Error> {
            let name = format!("releases.{}.json", plain_name(channel)?);
            *self.0.channel.lock().unwrap_or_else(|e| e.into_inner()) = Some(channel.to_owned());
            let json = self
                .0
                .read(&name, MAX_FEED)
                .map_err(|e| e.into_velopack(&name))?;
            Ok(serde_json::from_slice(&json)?)
        }

        fn download_release_entry(
            &self,
            asset: &VelopackAsset,
            local_file: &Path,
            progress: Option<Sender<i16>>,
        ) -> Result<(), Error> {
            let (length, reader) = self
                .0
                .open(&asset.FileName)
                .map_err(|e| e.into_velopack(&asset.FileName))?;
            if length.is_some_and(|n| n > asset.Size) {
                return Err(Error::SizeInvalid(
                    local_file.to_path_buf(),
                    asset.Size,
                    length.unwrap_or_default(),
                ));
            }
            let mut reader = reader.take(asset.Size + 1);
            let mut file = File::create(local_file)?;
            let mut buffer = vec![0; 1 << 20];
            let (mut received, mut reported) = (0u64, -1i16);
            loop {
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buffer[..n])?;
                received += n as u64;
                let percent = (received * 100 / asset.Size.max(1)).min(100) as i16;
                if percent != reported
                    && let Some(progress) = &progress
                {
                    let _ = progress.send(percent);
                    reported = percent;
                }
            }
            file.sync_all()?;
            Ok(())
        }
    }

    pub struct Velopack {
        manager: UpdateManager,
        feed: Arc<Feed>,
        packages: PathBuf,
    }

    impl Velopack {
        pub fn new() -> Result<Self, String> {
            let location = match std::env::var_os(FEED_FOLDER) {
                Some(folder) => Location::Folder(folder.into()),
                None => Location::Web(Https::default()),
            };
            let feed = Arc::new(Feed {
                location,
                channel: Mutex::new(None),
            });
            let config = locator_config()?;
            let packages = match &config {
                Some(config) => config.PackagesDir.clone(),
                None => locator::auto_locate_app_manifest(LocationContext::FromCurrentExe)
                    .map_err(|e| e.to_string())?
                    .get_packages_dir(),
            };
            let options = UpdateOptions {
                AllowVersionDowngrade: false,
                ..UpdateOptions::default()
            };
            let manager = UpdateManager::new(Source(feed.clone()), Some(options), config)
                .map_err(|e| e.to_string())?;
            Ok(Self {
                manager,
                feed,
                packages,
            })
        }

        fn release<'a>(offer: &'a Offer) -> Release<'a> {
            Release {
                channel: &offer.channel,
                version: &offer.version,
                file_name: &offer.file_name,
                sha256: &offer.sha256,
            }
        }

        fn check_package(&self, offer: &Offer) -> Result<(), Problem> {
            update::check_package(&self.packages.join(&offer.file_name), &Self::release(offer))
                .map_err(|e| match e {
                    PackageError::Io(e) => Problem::Failed(e.to_string()),
                    PackageError::Refused(r) => Problem::Refused(r),
                })
        }
    }

    fn asset(offer: &Offer) -> VelopackAsset {
        VelopackAsset {
            PackageId: offer.package_id.clone(),
            Version: offer.version.clone(),
            Type: "Full".into(),
            FileName: offer.file_name.clone(),
            SHA256: offer.sha256.clone(),
            Size: offer.size,
            ..VelopackAsset::default()
        }
    }

    fn failed(e: impl ToString) -> Problem {
        Problem::Failed(e.to_string())
    }

    impl Backend for Velopack {
        fn check(&self) -> Result<Option<Offer>, Problem> {
            let UpdateCheck::UpdateAvailable(info) =
                self.manager.check_for_updates().map_err(failed)?
            else {
                return Ok(None);
            };
            let target = info.TargetFullRelease;
            let newer = semver::Version::parse(&target.Version)
                .is_ok_and(|v| v > self.manager.get_current_version());
            if !newer {
                return Ok(None);
            }
            let channel = self
                .feed
                .channel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .ok_or_else(|| failed("the feed was read without a channel"))?;
            let signature_name = format!("{}.sig", target.FileName);
            let signature = match self.feed.read(&signature_name, MAX_SIGNATURE) {
                Ok(bytes) => String::from_utf8(bytes).map_err(|_| Refusal::MalformedSignature)?,
                Err(OpenError::Missing) => return Err(Refusal::NotSigned.into()),
                Err(OpenError::Other(e)) => return Err(Problem::Failed(e)),
            };
            let offer = Offer {
                version: target.Version,
                channel,
                package_id: target.PackageId,
                file_name: target.FileName,
                sha256: target.SHA256,
                size: target.Size,
            };
            update::check(&Self::release(&offer), &signature, PINNED_KEY)?;
            plain_name(&offer.file_name).map_err(failed)?;
            Ok(Some(offer))
        }

        fn download(&self, offer: &Offer, progress: &(dyn Fn(u8) + Sync)) -> Result<(), Problem> {
            // Velopack skips the download when a file of that name is there and never hashes it.
            let path = self.packages.join(&offer.file_name);
            if path.exists() && self.check_package(offer).is_err() {
                std::fs::remove_file(&path).map_err(failed)?;
            }
            // No base release, so no deltas: Velopack does not hash the package it rebuilds from
            // them, and only the full package is the one the signature names.
            let info = UpdateInfo {
                TargetFullRelease: asset(offer),
                ..UpdateInfo::default()
            };
            let (send, received) = std::sync::mpsc::channel::<i16>();
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    for percent in received {
                        progress(percent.clamp(0, 100) as u8);
                    }
                });
                self.manager.download_updates(&info, Some(send))
            })
            .map_err(|e| match e {
                Error::ChecksumInvalid(_, expected, actual) => {
                    Refusal::PackageDiffers { expected, actual }.into()
                }
                e => failed(e),
            })?;
            self.check_package(offer)
        }

        fn apply(&self, offer: &Offer) -> Result<(), Problem> {
            self.check_package(offer)?;
            self.manager
                .wait_exit_then_apply_updates(asset(offer), false, true, Vec::<String>::new())
                .map_err(failed)
        }
    }

    /// Velopack keeps an AppImage's downloads in `/var/tmp/velopack`, which every account on the
    /// machine can write, so another user could leave a package there for this app to install.
    /// They go to this user's own cache folder instead.
    #[cfg(target_os = "linux")]
    pub fn locator_config() -> Result<Option<velopack::locator::VelopackLocatorConfig>, String> {
        let packages = cloakframe_engine::app_dirs::cache_dir()
            .ok_or("there is no cache folder for update packages")?
            .join("updates");
        let found = locator::auto_locate_app_manifest(LocationContext::FromCurrentExe)
            .map_err(|e| e.to_string())?;
        Ok(Some(velopack::locator::VelopackLocatorConfig {
            RootAppDir: found.get_root_dir(),
            UpdateExePath: found.get_update_path(),
            PackagesDir: packages,
            ManifestPath: found.get_current_bin_dir().join("sq.version"),
            CurrentBinaryDir: found.get_current_bin_dir(),
            IsPortable: found.get_is_portable(),
        }))
    }

    /// Velopack keeps the downloads in the installation folder, which is this user's own.
    #[cfg(target_os = "windows")]
    pub fn locator_config() -> Result<Option<velopack::locator::VelopackLocatorConfig>, String> {
        Ok(None)
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub use velopack_backend::locator_config;
