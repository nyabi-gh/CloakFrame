use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::SystemTime;

use cloakframe_core::item::{ItemState, PublishBasis, ReviewMode};
use cloakframe_engine::batch::{job_from_scan, session_from_report};
use cloakframe_engine::cancel::CancelToken;
use cloakframe_engine::detect::{DetectorHandle, NoDetections};
use cloakframe_engine::publish::remove_stale_temp_files;
use cloakframe_engine::review::{ReviewDecisions, apply};
use cloakframe_engine::run::{ItemProcessor, Job, RunReport, start};
use cloakframe_engine::scan::{ScanRequest, scan};
use cloakframe_engine::session::{SessionId, SessionStore};

const USAGE: &str = "\
usage:
  cloakframe-cli run --output DIR [--review required|off] [--recursive] [--videos]
                     [--workers N] [--sessions DIR] [--synthetic] INPUT...
  cloakframe-cli review --session ID --decisions FILE [--sessions DIR] [--synthetic]
  cloakframe-cli sessions [--sessions DIR]
  cloakframe-cli --version

exit status: 0 done, 1 failed or refused items, 2 usage error, 3 items wait for review";

const DONE: u8 = 0;
const FAILED: u8 = 1;
const USAGE_ERROR: u8 = 2;
const NEEDS_REVIEW: u8 = 3;

struct Args {
    rest: std::vec::IntoIter<String>,
    positional: Vec<String>,
}

impl Args {
    fn value(&mut self, flag: &str) -> Result<String, String> {
        self.rest
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))
    }
}

#[derive(Default)]
struct Options {
    output: Option<PathBuf>,
    review: Option<ReviewMode>,
    recursive: bool,
    videos: bool,
    workers: Option<usize>,
    sessions: Option<PathBuf>,
    synthetic: bool,
    session: Option<String>,
    decisions: Option<PathBuf>,
    inputs: Vec<PathBuf>,
}

fn parse(args: Vec<String>) -> Result<Options, String> {
    let mut args = Args {
        rest: args.into_iter(),
        positional: Vec::new(),
    };
    let mut o = Options::default();
    while let Some(arg) = args.rest.next() {
        match arg.as_str() {
            "--output" => o.output = Some(args.value(&arg)?.into()),
            "--review" => {
                o.review = Some(match args.value(&arg)?.as_str() {
                    "required" => ReviewMode::Required,
                    "off" => ReviewMode::Off,
                    other => return Err(format!("--review is required or off, not {other}")),
                })
            }
            "--recursive" => o.recursive = true,
            "--videos" => o.videos = true,
            "--workers" => {
                let value = args.value(&arg)?;
                o.workers =
                    Some(value.parse().ok().filter(|&n| n > 0).ok_or_else(|| {
                        format!("--workers needs a positive number, not {value}")
                    })?);
            }
            "--sessions" => o.sessions = Some(args.value(&arg)?.into()),
            "--synthetic" => o.synthetic = true,
            "--session" => o.session = Some(args.value(&arg)?),
            "--decisions" => o.decisions = Some(args.value(&arg)?.into()),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => args.positional.push(arg),
        }
    }
    o.inputs = args.positional.into_iter().map(PathBuf::from).collect();
    Ok(o)
}

fn session_store(o: &Options) -> Result<SessionStore, String> {
    if let Some(dir) = &o.sessions {
        return Ok(SessionStore::new(dir));
    }
    cloakframe_engine::app_dirs::data_dir()
        .map(|dir| SessionStore::new(dir.join("sessions")))
        .ok_or_else(|| "no app data folder for this user; pass --sessions DIR".into())
}

/// The media pipelines arrive in M2 (photos) and M4 (video); until then only the synthetic
/// processor can run, and only in builds with the `synthetic` feature.
fn processor(o: &Options) -> Result<Arc<dyn ItemProcessor>, String> {
    #[cfg(feature = "synthetic")]
    if o.synthetic {
        return Ok(Arc::new(
            cloakframe_engine::synthetic::SyntheticProcessor::default(),
        ));
    }
    let _ = o;
    Err("this build has no media pipeline yet".into())
}

fn detector() -> Result<DetectorHandle, String> {
    DetectorHandle::spawn(Box::new(NoDetections))
        .map_err(|e| format!("cannot start the inference thread: {e}"))
}

fn state_label(state: ItemState) -> &'static str {
    match state {
        ItemState::Published(PublishBasis::AutomaticUnreviewed) => {
            "published (automatic, unreviewed)"
        }
        ItemState::Published(PublishBasis::Reviewed) => "published (reviewed)",
        ItemState::Published(PublishBasis::ExportedWithWarnings) => "exported with warnings",
        ItemState::NeedsReview => "needs review",
        ItemState::Failed => "failed",
        ItemState::Cancelled => "cancelled",
        _ => "unfinished",
    }
}

fn print_report(report: &RunReport) {
    for o in &report.items {
        let mut line = format!(
            "{}\t{}\t{}",
            o.item.0,
            state_label(o.state),
            o.input.display()
        );
        if let Some(failure) = &o.failure {
            line.push_str(&format!("\t{failure}"));
        }
        println!("{line}");
    }
}

fn exit_for(report: &RunReport) -> u8 {
    if report.count(|s| matches!(s, ItemState::Failed | ItemState::Cancelled)) > 0 {
        FAILED
    } else if report.count(|s| s == ItemState::NeedsReview) > 0 {
        NEEDS_REVIEW
    } else {
        DONE
    }
}

fn execute(job: Job, processor: Arc<dyn ItemProcessor>) -> Result<RunReport, String> {
    let handle =
        start(job, processor, detector()?).map_err(|e| format!("cannot start the run: {e}"))?;
    Ok(handle.wait())
}

fn clean_stale(job: &Job) {
    let mut folders: Vec<_> = job
        .items
        .iter()
        .filter_map(|i| i.destination.parent().map(PathBuf::from))
        .collect();
    folders.sort();
    folders.dedup();
    for folder in folders {
        let _ = remove_stale_temp_files(&folder, SystemTime::now());
    }
}

fn run(o: Options) -> Result<u8, String> {
    let output = o.output.clone().ok_or("run needs --output DIR")?;
    if o.inputs.is_empty() {
        return Err("run needs at least one input".into());
    }
    let processor = processor(&o)?;
    let store = session_store(&o)?;
    let scanned = scan(
        &ScanRequest {
            inputs: o.inputs.clone(),
            recursive: o.recursive,
            include_videos: o.videos,
            output: Some(output.clone()),
        },
        &CancelToken::new(),
    )
    .map_err(|_| "scan cancelled")?;
    for issue in &scanned.issues {
        eprintln!("cannot read {}: {:?}", issue.path.display(), issue.kind);
    }
    for (extension, count) in &scanned.skipped {
        let shown = if extension.is_empty() {
            "(none)"
        } else {
            extension
        };
        eprintln!("skipped {count} file(s) of unsupported type {shown}");
    }
    let workers = o
        .workers
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let (job, plan) = job_from_scan(
        &scanned.files,
        &output,
        o.review.unwrap_or(ReviewMode::Required),
        workers,
    );
    clean_stale(&job);
    let fingerprint = processor.fingerprint();
    let report = execute(job, processor)?;
    print_report(&report);
    if let Some(session) = session_from_report(&report, &plan, &output, fingerprint)
        .map_err(|e| format!("cannot prepare the review session: {e}"))?
    {
        let id = SessionId::generate();
        store
            .save(&id, &session)
            .map_err(|e| format!("cannot save the review session: {e}"))?;
        println!("session\t{}", id.as_str());
    }
    let mut code = exit_for(&report);
    if code == DONE && !scanned.issues.is_empty() {
        code = FAILED;
    }
    Ok(code)
}

fn review(o: Options) -> Result<u8, String> {
    let id = o
        .session
        .as_deref()
        .and_then(SessionId::parse)
        .ok_or("review needs --session ID (as printed by run)")?;
    let decisions_path = o.decisions.clone().ok_or("review needs --decisions FILE")?;
    let processor = processor(&o)?;
    let store = session_store(&o)?;
    let mut session = store.load(&id).map_err(|e| e.to_string())?;
    let decisions: ReviewDecisions = serde_json::from_slice(
        &std::fs::read(&decisions_path).map_err(|e| format!("cannot read the decisions: {e}"))?,
    )
    .map_err(|e| format!("the decisions file is not valid: {e}"))?;

    let applied = apply(&mut session, &decisions);
    for (item, rejection) in &applied.rejected {
        println!("{}\trefused\t{rejection}", item.0);
    }
    let mut code = if applied.rejected.is_empty() {
        DONE
    } else {
        FAILED
    };
    if !applied.jobs.is_empty() {
        let job = Job {
            items: applied.jobs,
            review: ReviewMode::Required,
            workers: o.workers.unwrap_or(1),
        };
        clean_stale(&job);
        let report = execute(job, processor)?;
        print_report(&report);
        let finished: Vec<_> = report
            .items
            .iter()
            .filter(|o| matches!(o.state, ItemState::Published(_)))
            .map(|o| o.item)
            .collect();
        session.items.retain(|i| !finished.contains(&i.item));
        code = code.max(exit_for(&report));
    }
    if session.items.is_empty() {
        store
            .remove(&id)
            .map_err(|e| format!("cannot remove the finished session: {e}"))?;
    } else {
        store
            .save(&id, &session)
            .map_err(|e| format!("cannot save the review session: {e}"))?;
        if code == DONE {
            code = NEEDS_REVIEW;
        }
    }
    Ok(code)
}

fn sessions(o: Options) -> Result<u8, String> {
    let store = session_store(&o)?;
    for id in store
        .list()
        .map_err(|e| format!("cannot list sessions: {e}"))?
    {
        match store.load(&id) {
            Ok(s) => println!(
                "{}\t{} item(s)\t{}",
                id.as_str(),
                s.items.len(),
                s.output.display()
            ),
            Err(e) => println!("{}\tunreadable\t{e}", id.as_str()),
        }
    }
    Ok(DONE)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let rest: Vec<String> = args.collect();
    let result = match command.as_deref() {
        Some("--version") => {
            println!("cloakframe-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(DONE)
        }
        Some(c @ ("run" | "review" | "sessions")) => parse(rest).and_then(|o| match c {
            "run" => run(o),
            "review" => review(o),
            _ => sessions(o),
        }),
        _ => Err(String::new()),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            if !message.is_empty() {
                eprintln!("error: {message}");
            }
            eprintln!("{USAGE}");
            ExitCode::from(USAGE_ERROR)
        }
    }
}
