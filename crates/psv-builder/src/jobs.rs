use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use psv_setup::SetupError;
use psv_setup::minecraft::TARGET_VERSION;
use psv_setup::prepare::{self, Event, PrepareRequest};
use psv_setup::report::{self, Check, Inputs, Status};

use crate::pack;
use crate::paths::AppPaths;
use crate::payload::Payload;

#[derive(Debug)]
pub enum JobEvent {
    Progress(f32),
    Checks(Vec<Check>),
    Finished(Result<String, String>),
}

pub struct JobHandle {
    pub name: &'static str,
    pub cancel: Arc<AtomicBool>,
    pub events: Receiver<JobEvent>,
}

impl JobHandle {
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn log_checks(checks: &[Check]) {
    for check in checks {
        match check.status {
            Status::Pass => {
                tracing::info!(target: "check", "ok    {}: {}", check.name, check.detail)
            }
            Status::Warn => {
                tracing::warn!(target: "check", "warn  {}: {}", check.name, check.detail)
            }
            Status::Fail => {
                tracing::error!(target: "check", "FAIL  {}: {}", check.name, check.detail)
            }
        }
    }
}

pub fn run_checks(inputs: &Inputs) -> Vec<Check> {
    tracing::info!(target: "check", "checking setup for Minecraft {TARGET_VERSION}");
    let checks = report::run_checks(inputs);
    log_checks(&checks);
    checks
}

pub fn spawn_check(inputs: Inputs) -> JobHandle {
    spawn("check", move |sender, _cancel| {
        let checks = run_checks(&inputs);
        let failures = report::blocking_failures(&checks);
        let _ = sender.send(JobEvent::Checks(checks));
        if failures == 0 {
            Ok("setup looks good".to_owned())
        } else {
            Err(format!("{failures} check(s) failed"))
        }
    })
}

pub fn spawn_prepare(inputs: Inputs, paths: AppPaths, payload: Payload) -> JobHandle {
    spawn("prepare", move |sender, cancel| {
        let progress = |fraction: f32| {
            let _ = sender.send(JobEvent::Progress(fraction));
        };
        prepare_blocking(&inputs, &paths, &payload, cancel, progress)
            .map(|output| format!("game data prepared in {}", output.display()))
            .map_err(|error| error.to_string())
    })
}

pub fn spawn_pack_probe(output_dir: PathBuf, payload: Payload) -> JobHandle {
    spawn("pack", move |_sender, _cancel| {
        pack::build_probe_vpk(&payload.eboot(), &output_dir)
            .map(|path| format!("probe package written to {}", path.display()))
            .map_err(|error| error.to_string())
    })
}

fn spawn(
    name: &'static str,
    work: impl FnOnce(&Sender<JobEvent>, &AtomicBool) -> Result<String, String> + Send + 'static,
) -> JobHandle {
    let (sender, events) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    std::thread::spawn(move || {
        let result = work(&sender, &worker_cancel);
        match &result {
            Ok(message) => tracing::info!(target: "job", "{name} finished: {message}"),
            Err(message) => tracing::error!(target: "job", "{name} failed: {message}"),
        }
        let _ = sender.send(JobEvent::Finished(result));
    });
    JobHandle {
        name,
        cancel,
        events,
    }
}

/// Runs the full preparation: pre-flight checks, staging the lock file, then Horizon's
/// preparer. Returns the folder holding the prepared game data.
pub fn prepare_blocking(
    inputs: &Inputs,
    paths: &AppPaths,
    payload: &Payload,
    cancel: &AtomicBool,
    on_progress: impl Fn(f32),
) -> Result<PathBuf, SetupError> {
    let checks = run_checks(inputs);
    let failures = report::blocking_failures(&checks);
    if failures > 0 {
        return Err(SetupError::PrepareFailed(format!(
            "{failures} setup check(s) failed; fix them and try again"
        )));
    }

    let io_error = |path: &PathBuf| {
        let path = path.clone();
        move |source| SetupError::Io { path, source }
    };

    let root = paths.work_dir().join("prepare");
    std::fs::create_dir_all(&root).map_err(io_error(&root))?;
    let lock_source = payload.research_lock();
    let lock_target = root.join(crate::payload::RESEARCH_LOCK);
    std::fs::copy(&lock_source, &lock_target).map_err(io_error(&lock_source))?;

    let minecraft_dir = inputs
        .minecraft_dir
        .clone()
        .or_else(psv_setup::minecraft::default_minecraft_dir)
        .ok_or(SetupError::NoMinecraftDir)?;
    let server_jar = inputs
        .server_jar
        .clone()
        .ok_or_else(|| SetupError::PrepareFailed("no server jar selected".into()))?;
    let output = paths.work_dir().join("prepared");

    let request = PrepareRequest {
        program: payload.prepare_exe(),
        root,
        output: output.clone(),
        minecraft_dir,
        server_jar,
        java_home: inputs.java_home.clone(),
    };
    tracing::info!(target: "prepare", "starting {}", request.program.display());
    prepare::run(&request, cancel, |event| match event {
        Event::Message(text) => tracing::info!(target: "prepare", "{text}"),
        Event::Stderr(text) => tracing::info!(target: "prepare::stderr", "{text}"),
        Event::Progress(fraction) => {
            tracing::debug!(target: "prepare", "progress {:.0}%", fraction * 100.0);
            on_progress(fraction);
        }
    })?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_refuses_to_start_when_checks_fail() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data_dir: dir.path().join("data"),
        };
        let payload = Payload {
            dir: dir.path().join("payload"),
        };
        let inputs = Inputs {
            minecraft_dir: Some(dir.path().join("none")),
            ..Inputs::default()
        };
        let err = prepare_blocking(&inputs, &paths, &payload, &AtomicBool::new(false), |_| {})
            .unwrap_err();
        assert!(err.to_string().contains("setup check(s) failed"));
        assert!(
            !paths.work_dir().exists(),
            "nothing is staged after a failed pre-flight"
        );
    }

    #[test]
    fn check_job_reports_failures_and_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let handle = spawn_check(Inputs {
            minecraft_dir: Some(dir.path().to_path_buf()),
            ..Inputs::default()
        });
        let mut saw_checks = false;
        let finished = loop {
            match handle.events.recv().unwrap() {
                JobEvent::Checks(checks) => saw_checks = !checks.is_empty(),
                JobEvent::Finished(result) => break result,
                JobEvent::Progress(_) => {}
            }
        };
        assert!(saw_checks);
        assert!(finished.is_err());
    }
}
