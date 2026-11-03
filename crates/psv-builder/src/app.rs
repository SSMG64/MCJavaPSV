use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};
use psv_setup::report::{Check, Status};
use tracing::Level;

use crate::commands::{self, Command, HELP};
use crate::diagnostics;
use crate::jobs::{self, JobEvent, JobHandle};
use crate::logging::{ConsoleBuffer, ConsoleLine};
use crate::paths::AppPaths;
use crate::payload::Payload;
use crate::settings::Settings;

const REPAINT_INTERVAL: Duration = Duration::from_millis(100);

pub fn run(
    paths: AppPaths,
    payload: Payload,
    console: std::sync::Arc<ConsoleBuffer>,
) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([940.0, 720.0])
            .with_min_inner_size([680.0, 480.0])
            .with_title("MCJavaPSV Builder"),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "MCJavaPSV Builder",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(paths, payload, console)))),
    )
}

#[derive(Default)]
struct Fields {
    minecraft_dir: String,
    server_jar: String,
    java_home: String,
}

struct Filters {
    debug: bool,
    info: bool,
    warn: bool,
    error: bool,
}

pub struct App {
    paths: AppPaths,
    payload: Payload,
    console: std::sync::Arc<ConsoleBuffer>,
    fields: Fields,
    saved: Settings,
    checks: Vec<Check>,
    job: Option<JobHandle>,
    progress: f32,
    status: String,
    command_input: String,
    filters: Filters,
    stick_to_bottom: bool,
}

fn path_text(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

fn text_path(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim().trim_matches('"');
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn open_folder(path: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut command = std::process::Command::new(program);
    psv_setup::process::quiet(&mut command).arg(path);
    if let Err(error) = command.spawn() {
        tracing::warn!(target: "ui", "could not open {}: {error}", path.display());
    }
}

impl App {
    pub fn new(paths: AppPaths, payload: Payload, console: std::sync::Arc<ConsoleBuffer>) -> Self {
        let saved = Settings::load(&paths.settings_file());
        let mut app = Self {
            fields: Fields {
                minecraft_dir: path_text(&saved.minecraft_dir),
                server_jar: path_text(&saved.server_jar),
                java_home: path_text(&saved.java_home),
            },
            saved,
            paths,
            payload,
            console,
            checks: Vec::new(),
            job: None,
            progress: 0.0,
            status: "Ready".to_owned(),
            command_input: String::new(),
            filters: Filters {
                debug: false,
                info: true,
                warn: true,
                error: true,
            },
            stick_to_bottom: true,
        };
        if app.fields.minecraft_dir.is_empty() {
            app.auto_detect();
        }
        tracing::info!(target: "app", "MCJavaPSV Builder {} started", crate::VERSION);
        tracing::info!(target: "app", "logs are written to {}", app.paths.log_dir().display());
        app
    }

    fn settings(&self) -> Settings {
        Settings {
            minecraft_dir: text_path(&self.fields.minecraft_dir),
            server_jar: text_path(&self.fields.server_jar),
            java_home: text_path(&self.fields.java_home),
            output_dir: self.saved.output_dir.clone(),
        }
    }

    fn persist_if_changed(&mut self) {
        let current = self.settings();
        if current != self.saved {
            if let Err(error) = current.save(&self.paths.settings_file()) {
                tracing::warn!(target: "app", "could not save settings: {error}");
            }
            self.saved = current;
        }
    }

    fn auto_detect(&mut self) {
        match psv_setup::minecraft::default_minecraft_dir().filter(|dir| dir.is_dir()) {
            Some(dir) => {
                tracing::info!(target: "app", "found a Minecraft folder at {}", dir.display());
                self.fields.minecraft_dir = dir.display().to_string();
            }
            None => {
                tracing::info!(target: "app", "no default Minecraft folder was found; choose one")
            }
        }
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn start_check(&mut self) {
        if self.busy() {
            tracing::warn!(target: "ui", "a job is already running");
            return;
        }
        self.persist_if_changed();
        self.status = "Checking setup".to_owned();
        self.job = Some(jobs::spawn_check(self.settings().inputs(&self.payload)));
    }

    fn start_prepare(&mut self) {
        if self.busy() {
            tracing::warn!(target: "ui", "a job is already running");
            return;
        }
        self.persist_if_changed();
        self.progress = 0.0;
        self.status = "Preparing game data".to_owned();
        self.job = Some(jobs::spawn_prepare(
            self.settings().inputs(&self.payload),
            self.paths.clone(),
            self.payload.clone(),
        ));
    }

    fn poll_job(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let mut finished = false;
        loop {
            match job.events.try_recv() {
                Ok(JobEvent::Progress(fraction)) => self.progress = fraction,
                Ok(JobEvent::Checks(checks)) => self.checks = checks,
                Ok(JobEvent::Finished(result)) => {
                    self.status = match result {
                        Ok(message) => message,
                        Err(message) => format!("Failed: {message}"),
                    };
                    finished = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.job = None;
        }
    }

    fn run_command(&mut self, command: Command) {
        match command {
            Command::Help => HELP
                .lines()
                .for_each(|l| tracing::info!(target: "console", "{l}")),
            Command::Clear => self.console.clear(),
            Command::Check => self.start_check(),
            Command::Prepare => self.start_prepare(),
            Command::Cancel => match &self.job {
                Some(job) => {
                    job.request_cancel();
                    tracing::info!(target: "console", "cancelling {}", job.name);
                }
                None => tracing::info!(target: "console", "nothing is running"),
            },
            Command::Env => self.print_env(),
            Command::Logs => {
                tracing::info!(target: "console", "logs: {}", self.paths.log_dir().display());
            }
            Command::Export => self.export_diagnostics(),
            Command::Version => {
                tracing::info!(target: "console", "MCJavaPSV Builder {}", crate::VERSION);
            }
        }
    }

    fn print_env(&self) {
        let report = diagnostics::build_report(
            &self.settings(),
            &self.checks,
            &ConsoleBuffer::default(),
            &self.paths,
            &self.payload,
        );
        for line in report.lines().filter(|l| !l.trim().is_empty()) {
            tracing::info!(target: "env", "{line}");
        }
    }

    fn export_diagnostics(&self) {
        let report = diagnostics::build_report(
            &self.settings(),
            &self.checks,
            &self.console,
            &self.paths,
            &self.payload,
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let path = self.paths.data_dir.join(format!("diagnostics-{stamp}.txt"));
        let written = std::fs::create_dir_all(&self.paths.data_dir)
            .and_then(|()| std::fs::write(&path, report));
        match written {
            Ok(()) => tracing::info!(target: "console", "diagnostics saved to {}", path.display()),
            Err(error) => tracing::error!(target: "console", "could not save diagnostics: {error}"),
        }
    }

    fn path_row(
        ui: &mut egui::Ui,
        label: &str,
        hint: &str,
        value: &mut String,
    ) -> Option<RowAction> {
        let mut action = None;
        ui.label(label);
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        );
        if ui.button("Browse").clicked() {
            action = Some(RowAction::Browse);
        }
        ui.end_row();
        action
    }

    fn main_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("MCJavaPSV Builder");
        ui.label("Phase 0: check your setup and prepare your game data. Building the VPK arrives in a later phase.");
        ui.add_space(8.0);

        let mut browse_minecraft = false;
        let mut browse_server = false;
        let mut browse_java = false;
        egui::Grid::new("setup")
            .num_columns(3)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                browse_minecraft = Self::path_row(
                    ui,
                    "Minecraft folder",
                    "the official launcher's .minecraft folder",
                    &mut self.fields.minecraft_dir,
                )
                .is_some();
                browse_server = Self::path_row(
                    ui,
                    "Server jar",
                    "the official 26.3 server jar you downloaded",
                    &mut self.fields.server_jar,
                )
                .is_some();
                browse_java = Self::path_row(
                    ui,
                    "Java folder",
                    "optional: a Java 25 JDK, detected automatically if empty",
                    &mut self.fields.java_home,
                )
                .is_some();
            });

        if browse_minecraft && let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.fields.minecraft_dir = dir.display().to_string();
        }
        if browse_server
            && let Some(file) = rfd::FileDialog::new()
                .add_filter("Java archive", &["jar"])
                .pick_file()
        {
            self.fields.server_jar = file.display().to_string();
        }
        if browse_java && let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.fields.java_home = dir.display().to_string();
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Auto-detect").clicked() {
                self.auto_detect();
            }
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Check setup"))
                .clicked()
            {
                self.start_check();
            }
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Prepare game data"))
                .clicked()
            {
                self.start_prepare();
            }
            if ui
                .add_enabled(self.busy(), egui::Button::new("Cancel"))
                .clicked()
                && let Some(job) = &self.job
            {
                job.request_cancel();
            }
            ui.add_enabled(false, egui::Button::new("Build VPK"))
                .on_disabled_hover_text("Available in a later phase");
        });

        if self.busy() {
            ui.add(
                egui::ProgressBar::new(self.progress)
                    .show_percentage()
                    .animate(true),
            );
        }
        ui.label(&self.status);
        ui.add_space(8.0);

        if !self.checks.is_empty() {
            ui.separator();
            ui.strong("Setup checks");
            for check in &self.checks {
                let (tag, color) = match check.status {
                    Status::Pass => ("PASS", Color32::from_rgb(90, 190, 90)),
                    Status::Warn => ("WARN", Color32::from_rgb(230, 180, 60)),
                    Status::Fail => ("FAIL", Color32::from_rgb(230, 90, 90)),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tag).color(color).monospace());
                    ui.strong(check.name);
                    ui.label(&check.detail);
                });
            }
        }
    }

    fn visible_lines(&self) -> Vec<ConsoleLine> {
        self.console.with_lines(|lines| {
            lines
                .iter()
                .filter(|line| match line.level {
                    Level::ERROR => self.filters.error,
                    Level::WARN => self.filters.warn,
                    Level::INFO => self.filters.info,
                    _ => self.filters.debug,
                })
                .cloned()
                .collect()
        })
    }

    fn console_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Console");
            ui.checkbox(&mut self.filters.debug, "Debug");
            ui.checkbox(&mut self.filters.info, "Info");
            ui.checkbox(&mut self.filters.warn, "Warn");
            ui.checkbox(&mut self.filters.error, "Error");
            ui.checkbox(&mut self.stick_to_bottom, "Follow");
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(self.console.to_text());
            }
            if ui.button("Clear").clicked() {
                self.console.clear();
            }
            if ui.button("Open logs folder").clicked() {
                let dir = self.paths.log_dir();
                let _ = std::fs::create_dir_all(&dir);
                open_folder(&dir);
            }
        });

        let lines = self.visible_lines();
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        let input_height = 34.0;
        let height = (ui.available_height() - input_height).max(60.0);
        egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, false])
            .stick_to_bottom(self.stick_to_bottom)
            .show_rows(ui, row_height, lines.len(), |ui, range| {
                for line in &lines[range] {
                    let color = match line.level {
                        Level::ERROR => Color32::from_rgb(235, 100, 100),
                        Level::WARN => Color32::from_rgb(230, 180, 60),
                        Level::INFO => ui.visuals().text_color(),
                        _ => Color32::GRAY,
                    };
                    ui.label(RichText::new(line.render()).monospace().color(color));
                }
            });

        ui.horizontal(|ui| {
            ui.label(RichText::new(">").monospace());
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.command_input)
                    .hint_text("type help for commands")
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let input = std::mem::take(&mut self.command_input);
                match commands::parse(&input) {
                    Ok(Some(command)) => {
                        tracing::info!(target: "console", "> {}", input.trim());
                        self.run_command(command);
                    }
                    Ok(None) => {}
                    Err(message) => tracing::warn!(target: "console", "{message}"),
                }
                response.request_focus();
            }
        });
    }
}

enum RowAction {
    Browse,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        ctx.request_repaint_after(REPAINT_INTERVAL);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::bottom("console")
            .resizable(true)
            .default_size(300.0)
            .min_size(120.0)
            .show(ui, |ui| self.console_ui(ui));
        egui::CentralPanel::default().show(ui, |ui| self.main_ui(ui));
        self.persist_if_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_text_round_trips_and_trims_quotes() {
        assert_eq!(
            text_path("  \"C:\\Games\\mc\" "),
            Some(PathBuf::from("C:\\Games\\mc"))
        );
        assert_eq!(text_path("   "), None);
        assert_eq!(path_text(&None), "");
        assert_eq!(path_text(&Some("a".into())), "a");
    }
}
