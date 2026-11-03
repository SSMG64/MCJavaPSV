use std::fmt::Write as _;
use std::time::Duration;

use horizon_core::random::{LegacyRandom, RandomSource};

#[used]
#[unsafe(export_name = "_newlib_heap_size_user")]
static NEWLIB_HEAP_SIZE_USER: u32 = 192 * 1024 * 1024;

const SEED_ZERO_FIRST_INT: i32 = -1_155_484_576;
const REPORT_DIR: &str = "ux0:data/MCJavaPSV";

fn probe() -> String {
    let mut report = String::new();
    let first = LegacyRandom::new(0).next_int();
    let verdict = if first == SEED_ZERO_FIRST_INT { "ok" } else { "MISMATCH" };
    let parsed = horizon_world::nbt::parse_snbt("{answer:42}").is_ok();

    let _ = writeln!(report, "horizon target: {}", horizon_core::VANILLA_VERSION);
    let _ = writeln!(report, "Random(0).nextInt() = {first} ({verdict})");
    let _ = writeln!(report, "snbt parse: {}", if parsed { "ok" } else { "failed" });
    report
}

fn main() {
    let report = probe();
    print!("{report}");
    let _ = std::fs::create_dir_all(REPORT_DIR);
    let _ = std::fs::write(format!("{REPORT_DIR}/probe.txt"), &report);
    std::thread::sleep(Duration::from_secs(10));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_known_java_random_value() {
        assert!(probe().contains("(ok)"));
    }

    #[test]
    fn world_crate_links_and_parses() {
        assert!(probe().contains("snbt parse: ok"));
    }
}
