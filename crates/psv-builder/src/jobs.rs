use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use thiserror::Error;

use psv_bake::{BakeError, TreeSource};
use psv_setup::SetupError;
use psv_setup::minecraft::TARGET_VERSION;
use psv_setup::prepare::{self, Event, PrepareRequest};
use psv_setup::report::{self, Check, Inputs, Status};

use crate::pack::{self, PackError};
use crate::paths::AppPaths;
use crate::payload::Payload;

#[derive(Debug)]
pub enum JobEvent {
    Progress(f32),
    Checks(Vec<Check>),
    Finished(Result<String, String>),
}

#[derive(Debug, Error)]
pub enum BuildError {
    #[error(transparent)]
    Setup(#[from] SetupError),
    #[error(transparent)]
    Bake(#[from] BakeError),
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("there is no prepared game data at {}; run Prepare game data first", .0.display())]
    NothingPrepared(PathBuf),
}

const PREPARE_SHARE: f32 = 0.6;
const BAKE_SHARE: f32 = 0.35;

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

pub fn spawn_bake(paths: AppPaths) -> JobHandle {
    spawn("bake", move |sender, cancel| {
        let progress = |fraction: f32| {
            let _ = sender.send(JobEvent::Progress(fraction));
        };
        bake_blocking(&paths, cancel, progress)
            .map(|path| format!("data pack written to {}", path.display()))
            .map_err(|error| error.to_string())
    })
}

pub fn spawn_build(
    inputs: Inputs,
    paths: AppPaths,
    payload: Payload,
    output_dir: PathBuf,
) -> JobHandle {
    spawn("build", move |sender, cancel| {
        let progress = |fraction: f32| {
            let _ = sender.send(JobEvent::Progress(fraction));
        };
        build_blocking(&inputs, &paths, &payload, &output_dir, cancel, progress)
            .map(|path| format!("package written to {}", path.display()))
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

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Bakes the prepared game data into the Vita data pack and returns its path.
pub fn bake_blocking(
    paths: &AppPaths,
    cancel: &AtomicBool,
    on_progress: impl Fn(f32),
) -> Result<PathBuf, BuildError> {
    let prepared = paths.work_dir().join("prepared");
    if !prepared.is_dir() {
        return Err(BuildError::NothingPrepared(prepared));
    }
    let output = paths.work_dir().join("baked").join("data.pak");
    tracing::info!(target: "bake", "baking {}", prepared.display());

    let mut source = TreeSource::new(&prepared);
    let mut last_reported = 0;
    let stats = psv_bake::bake_to_file(&mut source, &output, cancel, &mut |done, total| {
        if total > 0 && (done == total || done >= last_reported + total.div_ceil(100)) {
            last_reported = done;
            on_progress(done as f32 / total as f32);
        }
    })?;

    for warning in &stats.warnings {
        tracing::warn!(target: "bake", "{warning}");
    }
    tracing::info!(
        target: "bake",
        "{} textures ({} indexed, {} raw), {} other files, {} skipped",
        stats.textures,
        stats.indexed_textures,
        stats.raw_textures,
        stats.files,
        stats.skipped
    );
    tracing::info!(
        target: "bake",
        "{:.1} MiB of source became a {:.1} MiB pack of {} entries",
        mib(stats.source_bytes),
        mib(stats.pack.file_bytes),
        stats.pack.entries
    );
    Ok(output)
}

/// The steps after preparing: bake the prepared data, then assemble the package.
pub fn finish_build(
    paths: &AppPaths,
    payload: &Payload,
    output_dir: &Path,
    cancel: &AtomicBool,
    on_progress: impl Fn(f32),
) -> Result<PathBuf, BuildError> {
    let data = bake_blocking(paths, cancel, |f| {
        on_progress(PREPARE_SHARE + f * BAKE_SHARE)
    })?;
    on_progress(PREPARE_SHARE + BAKE_SHARE);
    let vpk = pack::build_vpk(&payload.eboot(), &data, output_dir)?;
    on_progress(1.0);
    Ok(vpk)
}

/// The whole pipeline: checks, prepare, bake, pack. The executable is checked first so a
/// missing one does not waste the time the preparer takes.
pub fn build_blocking(
    inputs: &Inputs,
    paths: &AppPaths,
    payload: &Payload,
    output_dir: &Path,
    cancel: &AtomicBool,
    on_progress: impl Fn(f32),
) -> Result<PathBuf, BuildError> {
    pack::check_eboot(&payload.eboot())?;
    prepare_blocking(inputs, paths, payload, cancel, |f| {
        on_progress(f * PREPARE_SHARE)
    })?;
    finish_build(paths, payload, output_dir, cancel, on_progress)
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

    fn write_png(path: &Path, rgba: &[u8], size: u32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(rgba)
            .unwrap();
        std::fs::write(path, out).unwrap();
    }

    fn prepared_fixture(dir: &Path) -> AppPaths {
        let paths = AppPaths {
            data_dir: dir.join("data"),
        };
        let prepared = paths.work_dir().join("prepared");
        let pixels: Vec<u8> = (0..16 * 16)
            .flat_map(|i| [(i % 5) as u8, 9, 9, 255])
            .collect();
        write_png(
            &prepared.join("assets/minecraft/textures/block/a.png"),
            &pixels,
            16,
        );
        let models = prepared.join("assets/minecraft/models/block");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("a.json"), br#"{"parent":"x"}"#).unwrap();
        paths
    }

    #[test]
    fn bake_reports_progress_and_writes_a_readable_pack() {
        let dir = tempfile::tempdir().unwrap();
        let paths = prepared_fixture(dir.path());
        let seen = std::sync::Mutex::new(Vec::new());
        let out = bake_blocking(&paths, &AtomicBool::new(false), |f| {
            seen.lock().unwrap().push(f)
        })
        .unwrap();

        let mut pack = psv_pack::PackReader::open(std::fs::File::open(&out).unwrap()).unwrap();
        assert_eq!(pack.len(), 2);
        let blob = pack.read("assets/minecraft/textures/block/a.tex").unwrap();
        assert_eq!(psv_pack::texture::decode(&blob).unwrap().width, 16);
        assert_eq!(
            pack.read("assets/minecraft/models/block/a.json").unwrap(),
            br#"{"parent":"x"}"#
        );

        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.last(), Some(&1.0));
        assert!(
            seen.windows(2).all(|w| w[0] <= w[1]),
            "progress went backwards: {seen:?}"
        );
    }

    #[test]
    fn bake_without_prepared_data_says_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data_dir: dir.path().join("data"),
        };
        let err = bake_blocking(&paths, &AtomicBool::new(false), |_| {}).unwrap_err();
        assert!(matches!(err, BuildError::NothingPrepared(_)));
        assert!(err.to_string().contains("Prepare game data"));
    }

    #[test]
    fn finishing_a_build_produces_a_vpk_with_the_data_inside() {
        let dir = tempfile::tempdir().unwrap();
        let paths = prepared_fixture(dir.path());
        let payload = Payload {
            dir: dir.path().join("payload"),
        };
        std::fs::create_dir_all(&payload.dir).unwrap();
        std::fs::write(payload.eboot(), b"SCE\0fake").unwrap();

        let progress = std::sync::Mutex::new(Vec::new());
        let vpk = finish_build(
            &paths,
            &payload,
            &dir.path().join("out"),
            &AtomicBool::new(false),
            |f| {
                progress.lock().unwrap().push(f);
            },
        )
        .unwrap();

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&vpk).unwrap()).unwrap();
        let mut inner = Vec::new();
        std::io::Read::read_to_end(
            &mut archive.by_name(pack::DATA_PACK_PATH).unwrap(),
            &mut inner,
        )
        .unwrap();
        let pack = psv_pack::PackReader::open(std::io::Cursor::new(inner)).unwrap();
        assert_eq!(pack.len(), 2);

        let progress = progress.into_inner().unwrap();
        assert_eq!(progress.last(), Some(&1.0));
        assert!(progress.iter().all(|&f| f >= PREPARE_SHARE));
    }

    #[test]
    fn a_missing_executable_stops_the_build_before_any_work() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data_dir: dir.path().join("data"),
        };
        let payload = Payload {
            dir: dir.path().join("payload"),
        };
        let err = build_blocking(
            &Inputs::default(),
            &paths,
            &payload,
            dir.path(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(err, BuildError::Pack(PackError::MissingEboot(_))));
        assert!(!paths.work_dir().exists(), "nothing may be staged first");
    }
}
