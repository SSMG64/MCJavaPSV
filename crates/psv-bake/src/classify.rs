use psv_pack::CodecChoice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// OpenGL shaders: the Vita runs its own, written for its GPU.
    Shader,
    /// Java classes and sources, which the Vita build never reads.
    Code,
    /// Metadata of the archive or the operating system.
    Junk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Convert to a texture blob stored at this path.
    Texture(String),
    /// Store the file under its own path with this codec.
    File(CodecChoice),
    Skip(SkipReason),
}

const ALREADY_COMPRESSED: &[&str] = &["gz", "jar", "lz4", "mp3", "nbt", "ogg", "zip", "zst"];

fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (_, extension) = name.rsplit_once('.')?;
    Some(extension.to_ascii_lowercase())
}

pub fn classify(path: &str) -> Action {
    let name = path.rsplit('/').next().unwrap_or(path);
    if path.starts_with("META-INF/")
        || matches!(
            name,
            ".DS_Store" | "Thumbs.db" | "desktop.ini" | ".mcassetsroot"
        )
    {
        return Action::Skip(SkipReason::Junk);
    }
    match extension(path).as_deref() {
        Some("fsh" | "vsh" | "glsl") => Action::Skip(SkipReason::Shader),
        Some("class" | "java") => Action::Skip(SkipReason::Code),
        Some("png") => Action::Texture(format!("{}.tex", &path[..path.len() - 4])),
        Some(ext) if ALREADY_COMPRESSED.contains(&ext) => Action::File(CodecChoice::Store),
        _ => Action::File(CodecChoice::Auto),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pngs_become_textures_with_the_same_stem() {
        assert_eq!(
            classify("assets/minecraft/textures/block/stone.png"),
            Action::Texture("assets/minecraft/textures/block/stone.tex".into())
        );
        assert_eq!(classify("pack.PNG"), Action::Texture("pack.tex".into()));
    }

    #[test]
    fn mcmeta_next_to_a_png_is_an_ordinary_file() {
        assert_eq!(
            classify("assets/minecraft/textures/block/water.png.mcmeta"),
            Action::File(CodecChoice::Auto)
        );
    }

    #[test]
    fn compressed_formats_are_stored() {
        for path in ["a/b.ogg", "a/b.nbt", "a/unifont.zip", "x.GZ"] {
            assert_eq!(classify(path), Action::File(CodecChoice::Store), "{path}");
        }
    }

    #[test]
    fn unusable_files_are_skipped() {
        assert_eq!(
            classify("assets/minecraft/shaders/core/a.fsh"),
            Action::Skip(SkipReason::Shader)
        );
        assert_eq!(classify("a/b.vsh"), Action::Skip(SkipReason::Shader));
        assert_eq!(
            classify("net/minecraft/A.class"),
            Action::Skip(SkipReason::Code)
        );
        assert_eq!(
            classify("META-INF/MANIFEST.MF"),
            Action::Skip(SkipReason::Junk)
        );
        assert_eq!(
            classify("assets/.mcassetsroot"),
            Action::Skip(SkipReason::Junk)
        );
        assert_eq!(classify("a/Thumbs.db"), Action::Skip(SkipReason::Junk));
    }

    #[test]
    fn data_files_compress() {
        for path in [
            "data/minecraft/recipe/a.json",
            "assets/minecraft/lang/en_us.json",
            "noextension",
        ] {
            assert_eq!(classify(path), Action::File(CodecChoice::Auto), "{path}");
        }
    }
}
