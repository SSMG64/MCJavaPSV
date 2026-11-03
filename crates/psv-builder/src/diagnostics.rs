use std::fmt::Write as _;
use std::path::Path;

use psv_setup::report::{Check, Status};

use crate::logging::ConsoleBuffer;
use crate::paths::AppPaths;
use crate::payload::Payload;
use crate::settings::Settings;

const LOG_TAIL_LINES: usize = 300;

/// Replaces the user's home folder with `~` so a report can be shared without
/// revealing the account name.
pub fn redact(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home else {
        return text.to_owned();
    };
    let native = home.display().to_string();
    if native.len() < 4 {
        return text.to_owned();
    }
    let mut out = text.replace(&native, "~");
    let forward = native.replace('\\', "/");
    if forward != native {
        out = out.replace(&forward, "~");
    }
    out
}

pub fn build_report(
    settings: &Settings,
    checks: &[Check],
    console: &ConsoleBuffer,
    paths: &AppPaths,
    payload: &Payload,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "MCJavaPSV diagnostics");
    let _ = writeln!(out, "version: {}", crate::VERSION);
    let _ = writeln!(out, "target minecraft: {}", psv_setup::TARGET_VERSION);
    let _ = writeln!(
        out,
        "os: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out, "data dir: {}", paths.data_dir.display());
    let _ = writeln!(out, "payload dir: {}", payload.dir.display());
    let _ = writeln!(
        out,
        "prepare tool present: {}",
        payload.prepare_exe().is_file()
    );
    let _ = writeln!(
        out,
        "research lock present: {}",
        payload.research_lock().is_file()
    );
    let _ = writeln!(out, "eboot present: {}", payload.eboot().is_file());
    let _ = writeln!(out, "\nsettings:\n{settings:#?}");
    let _ = writeln!(out, "\nchecks:");
    if checks.is_empty() {
        let _ = writeln!(out, "  (not run)");
    }
    for check in checks {
        let mark = match check.status {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "FAIL",
        };
        let _ = writeln!(out, "  [{mark}] {}: {}", check.name, check.detail);
    }
    let _ = writeln!(
        out,
        "\nlast console lines:\n{}",
        console.tail_text(LOG_TAIL_LINES)
    );
    redact(&out, dirs::home_dir().as_deref())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn redacts_both_slash_styles() {
        let home = PathBuf::from("C:\\Users\\alice");
        let text = "C:\\Users\\alice\\AppData and C:/Users/alice/x";
        assert_eq!(redact(text, Some(&home)), "~\\AppData and ~/x");
    }

    #[test]
    fn short_or_missing_home_is_left_alone() {
        assert_eq!(redact("abc /", Some(Path::new("/"))), "abc /");
        assert_eq!(redact("abc", None), "abc");
    }

    #[test]
    fn report_lists_checks_and_marks_failures() {
        let checks = [Check {
            name: "Java",
            status: Status::Fail,
            detail: "missing".into(),
        }];
        let report = build_report(
            &Settings::default(),
            &checks,
            &ConsoleBuffer::default(),
            &AppPaths {
                data_dir: "d".into(),
            },
            &Payload { dir: "p".into() },
        );
        assert!(report.contains("[FAIL] Java: missing"));
        assert!(report.contains("target minecraft: 26.3"));
    }
}
