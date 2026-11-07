use std::path::{Path, PathBuf};

use psv_vpk::{ParamSfo, SfoError, VpkBuilder, VpkError};
use thiserror::Error;

pub const TITLE_ID: &str = "MCJVPSV01";
pub const PROBE_TITLE: &str = "MCJavaPSV Probe";
pub const PROBE_FILE_NAME: &str = "MCJavaPSV-probe.vpk";

const ELF_MAGIC: &[u8] = b"\x7fELF";

#[derive(Debug, Error)]
pub enum PackError {
    #[error(
        "there is no eboot.bin at {}; use a release or CI build, or build one as described in docs/BUILDING-PSV.md",
        .0.display()
    )]
    MissingEboot(PathBuf),
    #[error("{} is empty or an unconverted ELF file, not a Vita executable", .0.display())]
    NotAnEboot(PathBuf),
    #[error("could not read or write {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Sfo(#[from] SfoError),
    #[error(transparent)]
    Vpk(#[from] VpkError),
}

/// Builds an installable package holding only the probe executable and generic art.
/// It contains no Minecraft data, so it can be built without any game files.
pub fn build_probe_vpk(eboot: &Path, output_dir: &Path) -> Result<PathBuf, PackError> {
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| PackError::Io { path, source }
    };

    if !eboot.is_file() {
        return Err(PackError::MissingEboot(eboot.to_path_buf()));
    }
    let executable = std::fs::read(eboot).map_err(io_error(eboot))?;
    if executable.is_empty() || executable.starts_with(ELF_MAGIC) {
        return Err(PackError::NotAnEboot(eboot.to_path_buf()));
    }

    let sfo = ParamSfo::homebrew(TITLE_ID, PROBE_TITLE)?;
    let mut package = VpkBuilder::new(executable, &sfo);
    package.add_generic_livearea()?;

    std::fs::create_dir_all(output_dir).map_err(io_error(output_dir))?;
    let target = output_dir.join(PROBE_FILE_NAME);
    let partial = output_dir.join(format!("{PROBE_FILE_NAME}.part"));
    package.write_to_path(&partial)?;
    std::fs::rename(&partial, &target).map_err(io_error(&target))?;
    tracing::info!(target: "pack", "wrote {}", target.display());
    Ok(target)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use zip::ZipArchive;

    use super::*;

    fn fake_eboot(dir: &Path, bytes: &[u8]) -> PathBuf {
        let path = dir.join("eboot.bin");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn builds_a_package_with_the_executable_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let eboot = fake_eboot(dir.path(), b"SCE\0fake-self");
        let out = dir.path().join("out/nested");

        let vpk = build_probe_vpk(&eboot, &out).unwrap();
        assert_eq!(vpk, out.join(PROBE_FILE_NAME));
        assert!(!out.join(format!("{PROBE_FILE_NAME}.part")).exists());

        let mut archive = ZipArchive::new(std::fs::File::open(&vpk).unwrap()).unwrap();
        let mut payload = Vec::new();
        archive
            .by_name("eboot.bin")
            .unwrap()
            .read_to_end(&mut payload)
            .unwrap();
        assert_eq!(payload, b"SCE\0fake-self");

        let mut sfo = Vec::new();
        archive
            .by_name("sce_sys/param.sfo")
            .unwrap()
            .read_to_end(&mut sfo)
            .unwrap();
        let parsed = ParamSfo::parse(&sfo).unwrap();
        assert_eq!(
            parsed.get("TITLE_ID"),
            Some(&psv_vpk::Value::Text {
                text: TITLE_ID.into(),
                reserved: 12
            })
        );
        for required in [
            "sce_sys/icon0.png",
            "sce_sys/livearea/contents/template.xml",
        ] {
            assert!(archive.by_name(required).is_ok(), "{required} missing");
        }
    }

    #[test]
    fn missing_eboot_explains_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let err = build_probe_vpk(&dir.path().join("nope.bin"), dir.path()).unwrap_err();
        assert!(matches!(err, PackError::MissingEboot(_)));
        assert!(err.to_string().contains("BUILDING-PSV"));
    }

    #[test]
    fn rejects_empty_files_and_raw_elf() {
        let dir = tempfile::tempdir().unwrap();
        for bytes in [&b""[..], &b"\x7fELF\x01\x01"[..]] {
            let eboot = fake_eboot(dir.path(), bytes);
            assert!(matches!(
                build_probe_vpk(&eboot, dir.path()),
                Err(PackError::NotAnEboot(_))
            ));
        }
    }
}
