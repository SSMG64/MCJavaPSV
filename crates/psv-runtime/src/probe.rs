use std::fmt::{Display, Write as _};
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::time::Instant;

use horizon_core::noise::PerlinNoise;
use horizon_core::random::{LegacyRandom, RandomSource, XoroshiroRandom};

pub const SEED_ZERO_FIRST_INT: i32 = -1_155_484_576;
pub const NOISE_CHECKSUM: u64 = 0x2e9c_9cc9_113f_6ec3;

const NOISE_SEED: i64 = 0x5EED_C0DE;
const NOISE_GRID: u32 = 16;
const MIB: usize = 1 << 20;

pub struct Config {
    pub heap_configured_mib: usize,
    pub heap_probe_cap_mib: usize,
    pub random_draws: u32,
    pub noise_samples: u32,
    pub thread_samples: u32,
    pub threads: usize,
    pub io_mib: usize,
}

impl Config {
    pub fn vita(heap_configured_mib: usize) -> Self {
        Self {
            heap_configured_mib,
            heap_probe_cap_mib: heap_configured_mib + 64,
            random_draws: 2_000_000,
            noise_samples: 200_000,
            thread_samples: 100_000,
            threads: 3,
            io_mib: 8,
        }
    }

    pub fn quick() -> Self {
        Self {
            heap_configured_mib: 32,
            heap_probe_cap_mib: 32,
            random_draws: 20_000,
            noise_samples: 5_000,
            thread_samples: 2_000,
            threads: 2,
            io_mib: 1,
        }
    }
}

pub struct Report {
    file: Option<File>,
    text: String,
    pub failures: usize,
}

impl Report {
    pub fn new(file: Option<File>) -> Self {
        Self {
            file,
            text: String::new(),
            failures: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line(&mut self, line: impl Display) {
        let _ = writeln!(self.text, "{line}");
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }

    pub fn value(&mut self, key: &str, value: impl Display) {
        self.line(format_args!("{key}: {value}"));
    }

    pub fn check(&mut self, name: &str, ok: bool) {
        if !ok {
            self.failures += 1;
        }
        self.line(format_args!(
            "check {name}: {}",
            if ok { "ok" } else { "FAILED" }
        ));
    }
}

fn stage(report: &mut Report, name: &str, work: impl FnOnce(&mut Report)) {
    report.line(format_args!("-- {name}"));
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| work(report)));
    if outcome.is_err() {
        report.failures += 1;
        report.line(format_args!("stage {name}: PANICKED"));
    }
}

fn fnv1a(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn noise_checksum() -> u64 {
    let noise = PerlinNoise::new(&mut XoroshiroRandom::new(NOISE_SEED));
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for ix in 0..NOISE_GRID {
        for iy in 0..NOISE_GRID {
            for iz in 0..NOISE_GRID {
                let value = noise.get3(
                    f64::from(ix) * 0.37 + 0.123,
                    f64::from(iy) * 0.53 - 7.7,
                    f64::from(iz) * 0.29 + 101.01,
                );
                hash = fnv1a(hash, &value.to_bits().to_le_bytes());
            }
        }
    }
    hash
}

fn correctness(report: &mut Report) {
    report.value("horizon target", horizon_core::VANILLA_VERSION);
    report.check(
        "java_random_seed_0",
        LegacyRandom::new(0).next_int() == SEED_ZERO_FIRST_INT,
    );
    report.check(
        "snbt_parse",
        horizon_world::nbt::parse_snbt("{answer:42}").is_ok(),
    );

    let checksum = noise_checksum();
    report.value("noise checksum", format_args!("{checksum:#018x}"));
    report.check("noise_matches_desktop", checksum == NOISE_CHECKSUM);

    let tiny32 = black_box(f32::MIN_POSITIVE) / black_box(2.0_f32);
    let tiny64 = black_box(f64::MIN_POSITIVE) / black_box(2.0_f64);
    report.check("f32_denormals_preserved", tiny32 > 0.0);
    report.check("f64_denormals_preserved", tiny64 > 0.0);
}

fn heap(report: &mut Report, config: &Config) {
    let step = 8;
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    let mut total = 0;
    while total < config.heap_probe_cap_mib {
        let mut block: Vec<u8> = Vec::new();
        if block.try_reserve_exact(step * MIB).is_err() {
            break;
        }
        block.resize(step * MIB, 1);
        blocks.push(block);
        total += step;
    }
    report.value("heap configured MiB", config.heap_configured_mib);
    report.value("heap allocated and written MiB", total);
}

fn timed<R>(work: impl FnOnce() -> R) -> (R, f64) {
    let start = Instant::now();
    let result = work();
    (result, start.elapsed().as_secs_f64())
}

fn rate(report: &mut Report, name: &str, count: u32, seconds: f64) {
    report.line(format_args!(
        "{name}: {:.0} ms, {:.2} M/s",
        seconds * 1000.0,
        f64::from(count) / seconds / 1e6
    ));
}

fn noise_walk(samples: u32) -> f32 {
    let noise = PerlinNoise::new(&mut XoroshiroRandom::new(NOISE_SEED));
    let mut sum = 0.0_f32;
    for i in 0..samples {
        let t = f64::from(i);
        sum += noise.get3(t * 0.013, t * 0.007 + 3.0, t * 0.011 - 9.0);
    }
    sum
}

fn speed(report: &mut Report, config: &Config) {
    let draws = config.random_draws;
    let (sum, seconds) = timed(|| {
        let mut random = LegacyRandom::new(42);
        (0..draws).fold(0_i32, |acc, _| acc.wrapping_add(random.next_int()))
    });
    black_box(sum);
    rate(report, "legacy random next_int", draws, seconds);

    let (sum, seconds) = timed(|| {
        let mut random = XoroshiroRandom::new(42);
        (0..draws).fold(0_i64, |acc, _| acc.wrapping_add(random.next_long()))
    });
    black_box(sum);
    rate(report, "xoroshiro next_long", draws, seconds);

    let (sum, seconds) = timed(|| noise_walk(config.noise_samples));
    black_box(sum);
    rate(report, "perlin get3", config.noise_samples, seconds);
}

fn threads(report: &mut Report, config: &Config) {
    let samples = config.thread_samples;
    let (_, single) = timed(|| black_box(noise_walk(samples)));
    report.line(format_args!(
        "perlin on 1 thread: {:.0} ms",
        single * 1000.0
    ));

    let start = Instant::now();
    let outcome = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..config.threads {
            let spawned = std::thread::Builder::new()
                .stack_size(MIB)
                .spawn_scoped(scope, move || black_box(noise_walk(samples)));
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(error) => return Err(error.to_string()),
            }
        }
        for handle in handles {
            handle.join().map_err(|_| "a worker panicked".to_owned())?;
        }
        Ok(())
    });
    let wall = start.elapsed().as_secs_f64();
    match outcome {
        Ok(()) => {
            report.line(format_args!(
                "perlin on {} threads: {:.0} ms wall",
                config.threads,
                wall * 1000.0
            ));
            report.line(format_args!(
                "thread scaling: {:.2}x of {}x",
                single * config.threads as f64 / wall,
                config.threads
            ));
        }
        Err(message) => {
            report.failures += 1;
            report.line(format_args!("threads FAILED: {message}"));
        }
    }
}

fn storage(report: &mut Report, config: &Config, dir: &Path) {
    let path = dir.join("io.tmp");
    let chunk = vec![0xA5_u8; 64 * 1024];
    let chunks = config.io_mib * MIB / chunk.len();

    let (written, seconds) = timed(|| -> std::io::Result<()> {
        let mut file = File::create(&path)?;
        for _ in 0..chunks {
            file.write_all(&chunk)?;
        }
        file.sync_all()
    });
    match written {
        Ok(()) => report.line(format_args!(
            "storage write: {:.1} MiB/s",
            config.io_mib as f64 / seconds
        )),
        Err(error) => {
            report.failures += 1;
            report.line(format_args!("storage write FAILED: {error}"));
            return;
        }
    }

    let (read, seconds) = timed(|| -> std::io::Result<usize> {
        let mut file = File::open(&path)?;
        let mut buffer = vec![0_u8; chunk.len()];
        let mut total = 0;
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                return Ok(total);
            }
            total += n;
        }
    });
    match read {
        Ok(total) => report.line(format_args!(
            "storage read: {:.1} MiB/s ({} MiB)",
            total as f64 / MIB as f64 / seconds,
            total / MIB
        )),
        Err(error) => {
            report.failures += 1;
            report.line(format_args!("storage read FAILED: {error}"));
        }
    }
    let _ = fs::remove_file(&path);
}

/// Runs every stage, writing `probe.txt` into `dir` as it goes so that a crash still
/// leaves everything measured before it.
pub fn run(config: &Config, dir: &Path) -> Report {
    let file = fs::create_dir_all(dir)
        .ok()
        .and_then(|()| File::create(dir.join("probe.txt")).ok());
    let mut report = Report::new(file);
    report.line(format_args!(
        "MCJavaPSV probe {} ({})",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    ));

    stage(&mut report, "correctness", correctness);
    stage(&mut report, "heap", |r| heap(r, config));
    stage(&mut report, "speed", |r| speed(r, config));
    stage(&mut report, "threads", |r| threads(r, config));
    stage(&mut report, "storage", |r| storage(r, config, dir));

    let failures = report.failures;
    report.value("failures", failures);
    report.line("probe finished");
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_the_known_value() {
        assert_eq!(LegacyRandom::new(0).next_int(), SEED_ZERO_FIRST_INT);
    }

    #[test]
    fn noise_checksum_is_stable_across_runs() {
        assert_eq!(noise_checksum(), noise_checksum());
    }

    #[test]
    fn noise_checksum_matches_the_recorded_value() {
        assert_eq!(
            noise_checksum(),
            NOISE_CHECKSUM,
            "update NOISE_CHECKSUM to {:#018x}",
            noise_checksum()
        );
    }

    #[test]
    fn full_run_reports_every_stage_and_no_failures() {
        let dir = tempfile::tempdir().unwrap();
        let report = run(&Config::quick(), dir.path());
        for stage in ["correctness", "heap", "speed", "threads", "storage"] {
            assert!(
                report.text().contains(&format!("-- {stage}")),
                "{stage} missing"
            );
        }
        assert_eq!(report.failures, 0, "{}", report.text());
        assert!(report.text().ends_with("probe finished\n"));
        let written = std::fs::read_to_string(dir.path().join("probe.txt")).unwrap();
        assert_eq!(written, report.text());
        assert!(!dir.path().join("io.tmp").exists());
    }
}
