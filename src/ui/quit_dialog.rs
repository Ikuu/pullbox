//! Asks before quitting while a job is rewriting comics.

use egui::RichText;

pub enum QuitOutcome {
    /// Cancel the job and quit once the comic in progress is finished.
    StopAndQuit,
    KeepRunning,
}

pub fn show_quit_dialog(ctx: &egui::Context, job_title: &str) -> Option<QuitOutcome> {
    let modal = egui::Modal::new(egui::Id::new("quit_dialog")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading("Quit while a job is running?");
        ui.label(format!("{job_title} is still in progress."));
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Pullbox can stop after the comic it's working on and then quit. \
                 Quitting in the middle of a comic could leave it half-finished.",
            )
            .small()
            .weak(),
        );
        ui.separator();

        let mut outcome = None;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Stop and Quit").clicked() {
                outcome = Some(QuitOutcome::StopAndQuit);
            }
            if ui.button("Keep Running").clicked() {
                outcome = Some(QuitOutcome::KeepRunning);
            }
        });
        outcome
    });

    if modal.should_close() {
        return Some(QuitOutcome::KeepRunning);
    }
    modal.inner
}
