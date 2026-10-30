use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cloakframe_app::app::{AppState, Command};
use cloakframe_app::fonts;
use cloakframe_app::i18n::Language;
use cloakframe_app::settings::Settings;
use cloakframe_app::updates::{Backend, Offer, Problem, Stage, Updates};
use cloakframe_engine::update::Refusal;
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

mod support;
use support::RunTraced;

/// Answers each call with what the test set and counts the calls.
#[derive(Default)]
struct Fake {
    check: Mutex<Option<Result<Option<Offer>, Problem>>>,
    download: Mutex<Option<Result<(), Problem>>>,
    /// Held by a test to keep a check from answering.
    hold: Mutex<()>,
    calls: [AtomicUsize; 3],
}

impl Fake {
    fn calls(&self) -> [usize; 3] {
        self.calls.each_ref().map(|c| c.load(Ordering::Relaxed))
    }
}

impl Backend for Fake {
    fn check(&self) -> Result<Option<Offer>, Problem> {
        self.calls[0].fetch_add(1, Ordering::Relaxed);
        drop(self.hold.lock().unwrap());
        self.check.lock().unwrap().clone().unwrap_or(Ok(None))
    }

    fn download(&self, _: &Offer, progress: &(dyn Fn(u8) + Sync)) -> Result<(), Problem> {
        self.calls[1].fetch_add(1, Ordering::Relaxed);
        progress(50);
        self.download.lock().unwrap().clone().unwrap_or(Ok(()))
    }

    fn apply(&self, _: &Offer) -> Result<(), Problem> {
        self.calls[2].fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

fn offer() -> Offer {
    Offer {
        version: "2.0.1".into(),
        channel: "win".into(),
        package_id: "CloakFrameApp".into(),
        file_name: "CloakFrameApp-2.0.1-full.nupkg".into(),
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        size: 3,
    }
}

fn harness(fake: &Arc<Fake>, check_updates: bool) -> Harness<'static, AppState> {
    let mut state = AppState::new(
        Settings {
            language: Some(Language::En),
            check_updates,
            ..Settings::default()
        },
        None,
        Vec::new(),
    );
    state.updates = Updates::with(fake.clone());
    Harness::builder()
        .with_size(egui::vec2(1100.0, 760.0))
        .with_context_setup(|ctx| ctx.set_fonts(fonts::definitions(Language::En)))
        .build_ui_state(
            |ui, state: &mut AppState| {
                state.ui(ui);
                support::record(ui.ctx());
            },
            state,
        )
}

/// Steps while the update thread works, then lets the window settle.
fn settle(harness: &mut Harness<'_, AppState>) {
    harness.step();
    harness.state_mut().updates.wait();
    harness.run_traced();
}

#[test]
fn an_update_found_at_startup_is_installed_only_when_the_user_asks() {
    let fake = Arc::new(Fake::default());
    *fake.check.lock().unwrap() = Some(Ok(Some(offer())));
    let mut harness = harness(&fake, true);
    settle(&mut harness);
    harness.get_by_label("CloakFrame 2.0.1 is available.");
    assert_eq!(fake.calls(), [1, 0, 0]);

    harness.get_by_label("Update").click();
    settle(&mut harness);
    harness.get_by_label_contains("has been downloaded");
    assert_eq!(fake.calls(), [1, 1, 0]);

    harness.get_by_label("Restart Now").click();
    settle(&mut harness);
    assert_eq!(fake.calls(), [1, 1, 1]);
    assert_eq!(harness.state().updates.stage, Stage::Quitting);
}

#[test]
fn later_puts_the_update_away() {
    let fake = Arc::new(Fake::default());
    *fake.check.lock().unwrap() = Some(Ok(Some(offer())));
    let mut harness = harness(&fake, true);
    settle(&mut harness);
    harness.get_by_label("Later").click();
    harness.run_traced();
    assert!(harness.query_by_label_contains("is available").is_none());
    assert_eq!(fake.calls(), [1, 0, 0]);
}

#[test]
fn nothing_is_asked_at_startup_when_the_check_is_off() {
    let fake = Arc::new(Fake::default());
    *fake.check.lock().unwrap() = Some(Ok(Some(offer())));
    let mut harness = harness(&fake, false);
    settle(&mut harness);
    assert_eq!(fake.calls(), [0, 0, 0]);

    harness.state_mut().command(Command::CheckUpdates);
    settle(&mut harness);
    assert_eq!(fake.calls(), [1, 0, 0]);
    harness.get_by_label("CloakFrame 2.0.1 is available.");
}

#[test]
fn a_check_the_user_asks_for_answers_even_when_there_is_nothing() {
    let fake = Arc::new(Fake::default());
    let mut harness = harness(&fake, true);
    settle(&mut harness);
    assert!(harness.query_by_label_contains("newest version").is_none());

    harness.state_mut().command(Command::CheckUpdates);
    settle(&mut harness);
    harness.get_by_label_contains("You have the newest version");

    *fake.check.lock().unwrap() = Some(Err(Problem::Failed("offline".into())));
    harness.state_mut().command(Command::CheckUpdates);
    settle(&mut harness);
    harness.get_by_label("Could not check for updates: offline");
}

#[test]
fn a_refused_update_is_shown_and_never_downloaded() {
    let fake = Arc::new(Fake::default());
    *fake.check.lock().unwrap() = Some(Err(Problem::Refused(Refusal::NotSigned)));
    let mut harness = harness(&fake, true);
    settle(&mut harness);
    harness.get_by_label_contains("was refused because it could not be verified");
    assert_eq!(fake.calls(), [1, 0, 0]);

    *fake.check.lock().unwrap() = Some(Ok(Some(offer())));
    *fake.download.lock().unwrap() = Some(Err(Problem::Refused(Refusal::PackageDiffers {
        expected: "a".into(),
        actual: "b".into(),
    })));
    harness.state_mut().command(Command::CheckUpdates);
    settle(&mut harness);
    harness.get_by_label("Update").click();
    settle(&mut harness);
    harness.get_by_label_contains("was refused because it could not be verified");
    assert!(harness.query_by_label("Restart Now").is_none());
}

#[test]
fn a_build_that_cannot_update_says_why_when_asked() {
    let reason = "this build does not update itself";
    let mut state = AppState::new(
        Settings {
            language: Some(Language::En),
            ..Settings::default()
        },
        None,
        Vec::new(),
    );
    state.updates = Updates::unavailable(reason.into());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 760.0))
        .with_context_setup(|ctx| ctx.set_fonts(fonts::definitions(Language::En)))
        .build_ui_state(|ui, state: &mut AppState| state.ui(ui), state);
    harness.run_traced();
    assert!(harness.query_by_label_contains("updates").is_none());
    harness.state_mut().command(Command::CheckUpdates);
    harness.run_traced();
    harness.get_by_label(&format!("Could not check for updates: {reason}"));
}

#[test]
fn turning_the_check_off_drops_the_answer_still_coming() {
    let fake = Arc::new(Fake::default());
    *fake.check.lock().unwrap() = Some(Ok(Some(offer())));
    let held = fake.hold.lock().unwrap();
    let mut harness = harness(&fake, true);
    harness.step();
    assert_eq!(
        harness.state().updates.stage,
        Stage::Checking { asked: false }
    );
    harness
        .state_mut()
        .command(Command::CheckUpdatesOnStart(false));
    harness.step();
    drop(held);
    while fake.calls()[0] == 0 {
        std::thread::yield_now();
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    harness.run_traced();
    assert!(!harness.state().settings.check_updates);
    assert_eq!(harness.state().updates.stage, Stage::Idle);
    assert!(harness.query_by_label_contains("is available").is_none());
}
