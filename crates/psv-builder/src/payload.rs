use std::path::PathBuf;

use psv_setup::java::executable;

pub const PREPARE_TOOL: &str = "horizon-prepare";
pub const RESEARCH_LOCK: &str = "research-lock.json";
pub const EBOOT: &str = "eboot.bin";

/// The files that ship next to the app: Horizon's preparer, the lock file that pins the
/// Minecraft version it accepts, and the prebuilt Vita executable.
#[derive(Debug, Clone)]
pub struct Payload {
    pub dir: PathBuf,
}

impl Payload {
    pub fn locate() -> Self {
        let dir = std::env::var_os("MCJAVAPSV_PAYLOAD")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                let exe = std::env::current_exe().ok()?;
                Some(exe.parent()?.join("payload"))
            })
            .unwrap_or_else(|| PathBuf::from("payload"));
        Self { dir }
    }

    pub fn prepare_exe(&self) -> PathBuf {
        executable(&self.dir, PREPARE_TOOL)
    }

    pub fn research_lock(&self) -> PathBuf {
        self.dir.join(RESEARCH_LOCK)
    }

    pub fn eboot(&self) -> PathBuf {
        self.dir.join(EBOOT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_platform() {
        let payload = Payload {
            dir: PathBuf::from("p"),
        };
        let expected = if cfg!(windows) {
            "horizon-prepare.exe"
        } else {
            "horizon-prepare"
        };
        assert_eq!(payload.prepare_exe(), PathBuf::from("p").join(expected));
        assert_eq!(
            payload.research_lock(),
            PathBuf::from("p/research-lock.json")
        );
        assert_eq!(payload.eboot(), PathBuf::from("p/eboot.bin"));
    }
}
