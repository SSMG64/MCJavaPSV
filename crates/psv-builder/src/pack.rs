use std::path::{Path, PathBuf};

use psv_vpk::{ParamSfo, SfoError, VpkBuilder, VpkError};
use thiserror::Error;

pub const TITLE_ID: &str = "MCJVPSV01";
pub const PROBE_TITLE: &str = "MCJavaPSV Probe";
pub const PROBE_FILE_NAME: &str = "MCJavaPSV-probe.vpk";
pub const TITLE: &str = "MCJavaPSV";
pub const FILE_NAME: &str = "MCJavaPSV.vpk";
pub const DATA_PACK_PATH: &str = "data/data.pak";

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
    #[error("there is no baked data pack at {}", .0.display())]
    MissingData(PathBuf),
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

fn read_eboot(eboot: &Path) -> Result<Vec<u8>, PackError> {
    if !eboot.is_file() {
        return Err(PackError::MissingEboot(eboot.to_path_buf()));
    }
    let executable = std::fs::read(eboot).map_err(|source| PackError::Io {
        path: eboot.to_path_buf(),
        source,
    })?;
    if executable.is_empty() || executable.starts_with(ELF_MAGIC) {
        return Err(PackError::NotAnEboot(eboot.to_path_buf()));
    }
    Ok(executable)
}

/// Fails fast, before any slow work, when the executable cannot be packaged.
pub fn check_eboot(eboot: &Path) -> Result<(), PackError> {
    read_eboot(eboot).map(drop)
}

fn assemble(
    eboot: &Path,
    title: &str,
    file_name: &str,
    data_pack: Option<&Path>,
    output_dir: &Path,
) -> Result<PathBuf, PackError> {
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| PackError::Io { path, source }
    };
    let executable = read_eboot(eboot)?;
    if let Some(data) = data_pack
        && !data.is_file()
    {
        return Err(PackError::MissingData(data.to_path_buf()));
    }

    let sfo = ParamSfo::homebrew(TITLE_ID, title)?;
    let mut package = VpkBuilder::new(executable, &sfo);
    package.add_generic_livearea()?;
    if let Some(data) = data_pack {
        package.add_file(DATA_PACK_PATH, data)?;
    }

    std::fs::create_dir_all(output_dir).map_err(io_error(output_dir))?;
    let target = output_dir.join(file_name);
    let partial = output_dir.join(format!("{file_name}.part"));
    package.write_to_path(&partial)?;
    std::fs::rename(&partial, &target).map_err(io_error(&target))?;
    tracing::info!(target: "pack", "wrote {}", target.display());
    Ok(target)
}

/// Builds an installable package holding only the probe executable and generic art.
/// It contains no Minecraft data, so it can be built without any game files.
pub fn build_probe_vpk(eboot: &Path, output_dir: &Path) -> Result<PathBuf, PackError> {
    assemble(eboot, PROBE_TITLE, PROBE_FILE_NAME, None, output_dir)
}

/// Builds the full package: the Vita executable, generic art, and the baked data pack.
/// The result holds material derived from the player's own copy of the game.
pub fn build_vpk(eboot: &Path, data_pack: &Path, output_dir: &Path) -> Result<PathBuf, PackError> {
    assemble(eboot, TITLE, FILE_NAME, Some(data_pack), output_dir)
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
    fn full_package_carries_the_data_pack_uncompressed() {
        let dir = tempfile::tempdir().unwrap();
        let eboot = fake_eboot(dir.path(), b"SCE\0fake-self");
        let data = dir.path().join("data.pak");
        std::fs::write(&data, b"MCPK pretend pack contents".repeat(100)).unwrap();

        let vpk = build_vpk(&eboot, &data, &dir.path().join("out")).unwrap();
        assert_eq!(vpk.file_name().unwrap(), FILE_NAME);

        let mut archive = ZipArchive::new(std::fs::File::open(&vpk).unwrap()).unwrap();
        let entry = archive.by_name(DATA_PACK_PATH).unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        assert_eq!(entry.size(), 26 * 100);
        drop(entry);
        let mut sfo = Vec::new();
        archive
            .by_name("sce_sys/param.sfo")
            .unwrap()
            .read_to_end(&mut sfo)
            .unwrap();
        let parsed = ParamSfo::parse(&sfo).unwrap();
        assert_eq!(
            parsed.get("TITLE"),
            Some(&psv_vpk::Value::Text {
                text: TITLE.into(),
                reserved: 128
            })
        );
    }

    #[test]
    fn full_package_needs_its_data_pack() {
        let dir = tempfile::tempdir().unwrap();
        let eboot = fake_eboot(dir.path(), b"SCE\0x");
        let err = build_vpk(&eboot, &dir.path().join("none.pak"), dir.path()).unwrap_err();
        assert!(matches!(err, PackError::MissingData(_)));
        assert!(check_eboot(&eboot).is_ok());
        assert!(check_eboot(&dir.path().join("none")).is_err());
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
