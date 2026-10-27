//! Shared by the UI tests: `Harness::run` with what each frame saw when the window does not
//! settle in time. CI has failed this way now and then with no repaint cause given, and the
//! failures did not come back in hundreds of repeated runs.

use std::time::Instant;

use eframe::egui;
use egui_kittest::Harness;

fn trace_id() -> egui::Id {
    egui::Id::unique("frame-trace")
}

/// Notes what this frame saw; call it last in the harness's ui closure.
pub fn record(ctx: &egui::Context) {
    let id = trace_id();
    let since = ctx
        .data(|d| d.get_temp::<Instant>(id))
        .map_or_else(String::new, |s| format!(" at {:?}", s.elapsed()));
    let line = format!(
        "pass {}{since}: repaint asked {}, discarded {}, causes since the last pass {:?}",
        ctx.cumulative_pass_nr(),
        ctx.has_requested_repaint(),
        ctx.will_discard(),
        ctx.repaint_causes(),
    );
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<String>>(id).push(line));
}

pub trait RunTraced {
    fn run_traced(&mut self) -> u64;
}

impl<S> RunTraced for Harness<'_, S> {
    /// [`Harness::run`], printing the frames [`record`] noted if it fails.
    #[track_caller]
    fn run_traced(&mut self) -> u64 {
        let id = trace_id();
        self.ctx.data_mut(|d| {
            d.remove_temp::<Vec<String>>(id);
            d.insert_temp(id, Instant::now());
        });
        match self.try_run() {
            Ok(steps) => steps,
            Err(e) => {
                let frames = self
                    .ctx
                    .data_mut(|d| d.remove_temp::<Vec<String>>(id))
                    .unwrap_or_default();
                panic!("{e}\nWhat each frame saw:\n{}", frames.join("\n"));
            }
        }
    }
}
