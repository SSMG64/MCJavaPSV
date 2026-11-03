use std::path::PathBuf;

use crate::java;
use crate::minecraft::{self, Install, TARGET_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub minecraft_dir: Option<PathBuf>,
    pub server_jar: Option<PathBuf>,
    pub java_home: Option<PathBuf>,
    pub prepare_exe: Option<PathBuf>,
}

pub const REQUIRED_JAVA_MAJOR: u32 = 25;

fn check(name: &'static str, status: Status, detail: impl Into<String>) -> Check {
    Check {
        name,
        status,
        detail: detail.into(),
    }
}

pub fn run_checks(inputs: &Inputs) -> Vec<Check> {
    let mut checks = Vec::new();

    let minecraft_dir = inputs
        .minecraft_dir
        .clone()
        .or_else(minecraft::default_minecraft_dir);
    let install = match minecraft_dir {
        None => {
            checks.push(check(
                "Minecraft install",
                Status::Fail,
                "no folder given and no default location on this system",
            ));
            None
        }
        Some(dir) => match Install::inspect(&dir) {
            Err(error) => {
                checks.push(check("Minecraft install", Status::Fail, error.to_string()));
                None
            }
            Ok(install) => {
                checks.push(client_check(&install));
                Some(install)
            }
        },
    };

    checks.push(server_check(inputs, install.as_ref()));
    checks.push(java_check(inputs, install.as_ref()));
    checks.push(preparer_check(inputs));
    checks
}

fn client_check(install: &Install) -> Check {
    let detail = format!("{TARGET_VERSION} found in {}", install.dir.display());
    match (
        &install.client_sha1,
        minecraft::sha1_file(&install.client_jar),
    ) {
        (Some(expected), Ok(actual)) if *expected != actual => check(
            "Minecraft install",
            Status::Fail,
            format!(
                "the {TARGET_VERSION} client jar does not match the hash in its version file; reinstall it from the launcher"
            ),
        ),
        (_, Err(error)) => check(
            "Minecraft install",
            Status::Fail,
            format!("could not read the client jar: {error}"),
        ),
        (None, _) => check(
            "Minecraft install",
            Status::Warn,
            format!("{detail} (its version file lists no hash to verify against)"),
        ),
        _ => check("Minecraft install", Status::Pass, detail),
    }
}

fn server_check(inputs: &Inputs, install: Option<&Install>) -> Check {
    let name = "Server jar";
    let Some(path) = &inputs.server_jar else {
        return check(
            name,
            Status::Fail,
            format!(
                "download the {TARGET_VERSION} server jar from minecraft.net/download/server and select it"
            ),
        );
    };
    if !path.is_file() {
        return check(
            name,
            Status::Fail,
            format!("{} does not exist", path.display()),
        );
    }
    let actual = match minecraft::sha1_file(path) {
        Ok(actual) => actual,
        Err(error) => {
            return check(name, Status::Fail, format!("could not read it: {error}"));
        }
    };
    match install.and_then(|i| i.server_sha1.as_deref()) {
        Some(expected) if expected == actual => {
            check(name, Status::Pass, path.display().to_string())
        }
        Some(_) => check(
            name,
            Status::Fail,
            format!(
                "{} is not the official {TARGET_VERSION} server jar (hash mismatch)",
                path.display()
            ),
        ),
        None => check(
            name,
            Status::Warn,
            "selected, but there is no install to compare its hash against yet",
        ),
    }
}

fn java_check(inputs: &Inputs, install: Option<&Install>) -> Check {
    let name = "Java";
    let major = install
        .and_then(|i| i.java_major)
        .unwrap_or(REQUIRED_JAVA_MAJOR);
    let mut hints: Vec<PathBuf> = inputs.java_home.iter().cloned().collect();
    if let Some(install) = install {
        hints.extend(install.launcher_runtimes());
    }
    match java::find(major, &hints) {
        Ok(found) if found.has_compiler => check(
            name,
            Status::Pass,
            format!("JDK {} at {}", found.major, found.home.display()),
        ),
        Ok(found) => check(
            name,
            Status::Warn,
            format!(
                "only a runtime (Java {}) was found at {}; install a Java {major} JDK to compile the exporters",
                found.major,
                found.home.display()
            ),
        ),
        Err(error) => check(name, Status::Fail, error.to_string()),
    }
}

fn preparer_check(inputs: &Inputs) -> Check {
    let name = "Bundled preparer";
    match &inputs.prepare_exe {
        Some(path) if path.is_file() => check(name, Status::Pass, path.display().to_string()),
        Some(path) => check(name, Status::Fail, format!("{} is missing", path.display())),
        None => check(
            name,
            Status::Fail,
            "horizon-prepare was not found next to the app",
        ),
    }
}

pub fn blocking_failures(checks: &[Check]) -> usize {
    checks.iter().filter(|c| c.status == Status::Fail).count()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn empty_inputs_fail_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let checks = run_checks(&Inputs {
            minecraft_dir: Some(dir.path().to_path_buf()),
            ..Inputs::default()
        });
        assert_eq!(checks.len(), 4);
        assert_eq!(checks[0].status, Status::Fail);
        assert_eq!(checks[1].status, Status::Fail);
        assert!(blocking_failures(&checks) >= 3);
    }

    #[test]
    fn server_jar_hash_is_compared_with_the_version_file() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("versions/26.3");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("26.3.jar"), b"abc").unwrap();
        fs::write(
            folder.join("26.3.json"),
            r#"{"downloads":{"client":{"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d"},
                "server":{"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d"}}}"#,
        )
        .unwrap();
        let server = dir.path().join("server.jar");
        fs::write(&server, b"abc").unwrap();

        let mut inputs = Inputs {
            minecraft_dir: Some(dir.path().to_path_buf()),
            server_jar: Some(server.clone()),
            ..Inputs::default()
        };
        let checks = run_checks(&inputs);
        assert_eq!(checks[0].status, Status::Pass);
        assert_eq!(checks[1].status, Status::Pass);

        fs::write(&server, b"tampered").unwrap();
        inputs.server_jar = Some(server);
        assert_eq!(run_checks(&inputs)[1].status, Status::Fail);
    }

    #[test]
    fn corrupted_client_jar_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("versions/26.3");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("26.3.jar"), b"corrupt").unwrap();
        fs::write(
            folder.join("26.3.json"),
            r#"{"downloads":{"client":{"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d"}}}"#,
        )
        .unwrap();
        let checks = run_checks(&Inputs {
            minecraft_dir: Some(dir.path().to_path_buf()),
            ..Inputs::default()
        });
        assert_eq!(checks[0].status, Status::Fail);
    }
}
