use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::SetupError;
use crate::process::quiet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaInstall {
    pub home: PathBuf,
    pub major: u32,
    pub has_compiler: bool,
}

/// Reads the major version from the first line of `javac -version` or `java -version`,
/// accepting both `25.0.1` and the old `1.8.0_362` style.
pub fn parse_major(output: &str) -> Option<u32> {
    let mut words = output.split_whitespace();
    let version = loop {
        let word = words.next()?;
        if word == "javac" || word == "version" {
            break words.next()?;
        }
    };
    let mut parts = version.trim_matches('"').split(['.', '-', '+', '_']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

pub fn executable(bin: &Path, tool: &str) -> PathBuf {
    if cfg!(windows) {
        bin.join(format!("{tool}.exe"))
    } else {
        bin.join(tool)
    }
}

fn probe(home: &Path) -> Option<JavaInstall> {
    let bin = home.join("bin");
    let java = executable(&bin, "java");
    let javac = executable(&bin, "javac");
    if !java.is_file() {
        return None;
    }
    let has_compiler = javac.is_file();
    let tool = if has_compiler { &javac } else { &java };
    let mut command = Command::new(tool);
    quiet(&mut command)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    Some(JavaInstall {
        home: home.to_path_buf(),
        major: parse_major(&text)?,
        has_compiler,
    })
}

fn installer_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut env_path = |name: &str, suffixes: &[&str]| {
        if let Some(base) = std::env::var_os(name).filter(|v| !v.is_empty()) {
            let base = PathBuf::from(base);
            roots.extend(suffixes.iter().map(|s| base.join(s)));
        }
    };
    const WINDOWS_VENDORS: &[&str] = &[
        "Eclipse Adoptium",
        "Java",
        "Microsoft",
        "Zulu",
        "BellSoft",
        "Amazon Corretto",
        "Semeru",
        "Oracle",
    ];
    env_path("ProgramFiles", WINDOWS_VENDORS);
    env_path(
        "LOCALAPPDATA",
        &["Programs/Eclipse Adoptium", "Programs/Microsoft"],
    );
    if cfg!(target_os = "linux") {
        roots.push(PathBuf::from("/usr/lib/jvm"));
    }
    if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
    }
    roots
}

fn homes_below(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut homes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        for candidate in [path.clone(), path.join("Contents/Home")] {
            if executable(&candidate.join("bin"), "java").is_file() {
                homes.push(candidate);
                break;
            }
        }
    }
    homes.sort();
    homes
}

fn path_homes() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .filter(|dir| executable(dir, "java").is_file())
        .filter_map(|dir| {
            let real = dir.canonicalize().unwrap_or(dir);
            (real.file_name()? == "bin").then(|| real.parent().map(Path::to_path_buf))?
        })
        .collect()
}

/// Every Java home worth probing, in the order the app prefers them.
pub fn candidate_homes(hints: &[PathBuf]) -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = hints.to_vec();
    if let Some(java_home) = std::env::var_os("JAVA_HOME").filter(|v| !v.is_empty()) {
        homes.push(PathBuf::from(java_home));
    }
    homes.extend(path_homes());
    for root in installer_roots() {
        homes.extend(homes_below(&root));
    }
    let mut seen = Vec::new();
    homes.retain(|home| {
        let key = home.canonicalize().unwrap_or_else(|_| home.clone());
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    homes
}

/// Finds a Java of at least `major`. A full JDK is preferred; a runtime-only install
/// is returned when no JDK exists, with `has_compiler` false.
pub fn find(major: u32, hints: &[PathBuf]) -> Result<JavaInstall, SetupError> {
    let mut runtime_only = None;
    for home in candidate_homes(hints) {
        let Some(install) = probe(&home) else {
            continue;
        };
        if install.major < major {
            continue;
        }
        if install.has_compiler {
            return Ok(install);
        }
        runtime_only.get_or_insert(install);
    }
    runtime_only.ok_or(SetupError::JavaNotFound { major })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_versions() {
        assert_eq!(parse_major("javac 25.0.1"), Some(25));
        assert_eq!(parse_major("javac 25"), Some(25));
        assert_eq!(parse_major("javac 26-ea"), Some(26));
        assert_eq!(
            parse_major("openjdk version \"25.0.1\" 2026-10-21\nOpenJDK Runtime Environment"),
            Some(25)
        );
    }

    #[test]
    fn parses_legacy_versions() {
        assert_eq!(parse_major("javac 1.8.0_362"), Some(8));
        assert_eq!(parse_major("java version \"1.8.0_362\""), Some(8));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_major(""), None);
        assert_eq!(parse_major("command not found"), None);
        assert_eq!(parse_major("javac"), None);
        assert_eq!(parse_major("javac abc"), None);
    }

    #[test]
    fn hints_come_first_and_duplicates_collapse() {
        let dir = tempfile::tempdir().unwrap();
        let hint = dir.path().to_path_buf();
        let homes = candidate_homes(&[hint.clone(), hint.clone()]);
        assert_eq!(homes.first(), Some(&hint));
        assert_eq!(homes.iter().filter(|h| **h == hint).count(), 1);
    }

    #[test]
    fn empty_hint_dir_is_not_a_java() {
        let dir = tempfile::tempdir().unwrap();
        assert!(probe(dir.path()).is_none());
    }
}
