use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde::Deserialize;

use crate::error::SetupError;
use crate::process::quiet;

#[derive(Debug, Clone)]
pub struct PrepareRequest {
    pub program: PathBuf,
    pub root: PathBuf,
    pub output: PathBuf,
    pub minecraft_dir: PathBuf,
    pub server_jar: PathBuf,
    pub java_home: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Message(String),
    Progress(f32),
    Stderr(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Message(String),
    Fraction(f32),
    Error(String),
    Raw(String),
}

#[derive(Deserialize)]
struct JsonLine {
    message: Option<String>,
    fraction: Option<f32>,
    error: Option<String>,
}

pub fn build_args(request: &PrepareRequest) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--prepare".into(),
        "--progress".into(),
        "json".into(),
        "--root".into(),
        request.root.clone().into(),
        "--output".into(),
        request.output.clone().into(),
        "--minecraft-dir".into(),
        request.minecraft_dir.clone().into(),
        "--server-jar".into(),
        request.server_jar.clone().into(),
    ];
    if let Some(java_home) = &request.java_home {
        args.push("--java-home".into());
        args.push(java_home.clone().into());
    }
    args
}

/// Interprets one line of `horizon-prepare --progress json`.
pub fn parse_line(line: &str) -> Line {
    match serde_json::from_str::<JsonLine>(line) {
        Ok(JsonLine {
            error: Some(error), ..
        }) => Line::Error(error),
        Ok(JsonLine {
            message: Some(message),
            ..
        }) => Line::Message(message),
        Ok(JsonLine {
            fraction: Some(fraction),
            ..
        }) => Line::Fraction(fraction.clamp(0.0, 1.0)),
        _ => Line::Raw(line.to_owned()),
    }
}

enum Raw {
    Out(String),
    Err(String),
}

/// Runs the preparer to completion, reporting events as they arrive. Setting `cancel`
/// kills the child process and returns [`SetupError::Cancelled`].
pub fn run(
    request: &PrepareRequest,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(Event),
) -> Result<(), SetupError> {
    let mut command = Command::new(&request.program);
    quiet(&mut command)
        .args(build_args(request))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|source| SetupError::Spawn {
        program: request.program.clone(),
        source,
    })?;

    let (sender, receiver) = mpsc::channel();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let sender = sender.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(Raw::Out(line)).is_err() {
                    break;
                }
            }
        }));
    }
    if let Some(stderr) = child.stderr.take() {
        let sender = sender.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if sender.send(Raw::Err(line)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(sender);

    let mut reported_error = None;
    let mut last_stderr = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SetupError::Cancelled);
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Raw::Out(text)) => match parse_line(&text) {
                Line::Message(message) | Line::Raw(message) => on_event(Event::Message(message)),
                Line::Fraction(fraction) => on_event(Event::Progress(fraction)),
                Line::Error(error) => {
                    on_event(Event::Message(format!("error: {error}")));
                    reported_error = Some(error);
                }
            },
            Ok(Raw::Err(text)) => {
                last_stderr = Some(text.clone());
                on_event(Event::Stderr(text));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for reader in readers {
        let _ = reader.join();
    }

    let status = child.wait().map_err(|source| SetupError::Spawn {
        program: request.program.clone(),
        source,
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(SetupError::PrepareFailed(
            reported_error
                .or(last_stderr)
                .unwrap_or_else(|| format!("exited with {status}")),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PrepareRequest {
        PrepareRequest {
            program: "horizon-prepare".into(),
            root: "root".into(),
            output: "out".into(),
            minecraft_dir: "mc".into(),
            server_jar: "server.jar".into(),
            java_home: None,
        }
    }

    #[test]
    fn args_match_the_preparer_cli() {
        let args = build_args(&request());
        let text: Vec<_> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            text,
            [
                "--prepare",
                "--progress",
                "json",
                "--root",
                "root",
                "--output",
                "out",
                "--minecraft-dir",
                "mc",
                "--server-jar",
                "server.jar"
            ]
        );
    }

    #[test]
    fn java_home_is_passed_only_when_set() {
        let mut with_java = request();
        with_java.java_home = Some("jdk".into());
        let args = build_args(&with_java);
        assert_eq!(args[args.len() - 2], "--java-home");
        assert_eq!(args[args.len() - 1], "jdk");
    }

    #[test]
    fn parses_progress_lines() {
        assert_eq!(
            parse_line(r#"{"message": "hello"}"#),
            Line::Message("hello".into())
        );
        assert_eq!(parse_line(r#"{"fraction": 0.5}"#), Line::Fraction(0.5));
        assert_eq!(parse_line(r#"{"fraction": 7}"#), Line::Fraction(1.0));
        assert_eq!(
            parse_line(r#"{"error": "boom"}"#),
            Line::Error("boom".into())
        );
        assert_eq!(parse_line("plain text"), Line::Raw("plain text".into()));
        assert_eq!(parse_line("{}"), Line::Raw("{}".into()));
    }

    #[cfg(unix)]
    mod with_fake_preparer {
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        fn script(body: &str) -> (tempfile::TempDir, PrepareRequest) {
            let dir = tempfile::tempdir().unwrap();
            let program = dir.path().join("fake-prepare");
            std::fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut req = request();
            req.program = program;
            (dir, req)
        }

        #[test]
        fn streams_events_and_succeeds() {
            let (_dir, req) = script(
                r#"echo '{"message":"step one"}'; echo '{"fraction":0.25}'; echo oops >&2; echo '{"fraction":1}'"#,
            );
            let mut events = Vec::new();
            run(&req, &AtomicBool::new(false), |e| events.push(e)).unwrap();
            assert!(events.contains(&Event::Message("step one".into())));
            assert!(events.contains(&Event::Progress(0.25)));
            assert!(events.contains(&Event::Stderr("oops".into())));
            assert!(events.contains(&Event::Progress(1.0)));
        }

        #[test]
        fn failure_carries_the_reported_error() {
            let (_dir, req) = script(r#"echo '{"error":"no server jar"}'; exit 3"#);
            let err = run(&req, &AtomicBool::new(false), |_| {}).unwrap_err();
            assert!(matches!(err, SetupError::PrepareFailed(m) if m == "no server jar"));
        }

        #[test]
        fn failure_falls_back_to_stderr_then_status() {
            let (_dir, req) = script("echo bad >&2; exit 1");
            let err = run(&req, &AtomicBool::new(false), |_| {}).unwrap_err();
            assert!(matches!(err, SetupError::PrepareFailed(m) if m == "bad"));
        }

        #[test]
        fn cancel_kills_the_child() {
            let (_dir, req) = script("sleep 30");
            let cancel = AtomicBool::new(true);
            let started = std::time::Instant::now();
            let err = run(&req, &cancel, |_| {}).unwrap_err();
            assert!(matches!(err, SetupError::Cancelled));
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }

    #[test]
    fn missing_program_is_a_spawn_error() {
        let mut req = request();
        req.program = "/definitely/not/here".into();
        let err = run(&req, &AtomicBool::new(false), |_| {}).unwrap_err();
        assert!(matches!(err, SetupError::Spawn { .. }));
    }
}
