use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use clap::{Args, Parser, Subcommand};
use psv_builder::diagnostics;
use psv_builder::jobs;
use psv_builder::logging;
use psv_builder::paths::AppPaths;
use psv_builder::payload::Payload;
use psv_builder::settings::Settings;
use psv_setup::report;

#[derive(Parser)]
#[command(
    name = "mcjavapsv-cli",
    version,
    about = "Command line front end of the MCJavaPSV builder"
)]
struct Cli {
    /// Print debug messages too
    #[arg(long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Args, Clone, Default)]
struct Sources {
    /// The official launcher's game folder (default: the launcher's own)
    #[arg(long)]
    minecraft_dir: Option<PathBuf>,
    /// The official 26.3 server jar you downloaded
    #[arg(long)]
    server_jar: Option<PathBuf>,
    /// A Java 25 JDK folder (default: detected)
    #[arg(long)]
    java_home: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Verify the Minecraft install, server jar, Java and bundled files
    Check(Sources),
    /// Run Horizon's preparer on your own game files
    Prepare(Sources),
    /// Turn already prepared game data into the Vita data pack
    Bake,
    /// Run the whole pipeline: prepare, bake, and write the full VPK
    Build {
        #[command(flatten)]
        sources: Sources,
        /// Folder for the package (default: the saved output folder, else the current folder)
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Build the probe VPK (no Minecraft data) from the bundled eboot.bin
    PackProbe {
        /// Folder for the package (default: the saved output folder, else the current folder)
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Print the detected setup and system details
    Env(Sources),
    /// Print where the log files are kept
    Logs,
}

fn merged(paths: &AppPaths, sources: Sources) -> Settings {
    let saved = Settings::load(&paths.settings_file());
    Settings {
        minecraft_dir: sources.minecraft_dir.or(saved.minecraft_dir),
        server_jar: sources.server_jar.or(saved.server_jar),
        java_home: sources.java_home.or(saved.java_home),
        output_dir: saved.output_dir,
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = AppPaths::detect();
    let logging = logging::init(&paths.log_dir(), true, cli.verbose);
    let payload = Payload::locate();

    match cli.command {
        Cmd::Logs => {
            println!("{}", paths.log_dir().display());
            ExitCode::SUCCESS
        }
        Cmd::Check(sources) => {
            let inputs = merged(&paths, sources).inputs(&payload);
            let checks = jobs::run_checks(&inputs);
            if report::blocking_failures(&checks) == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Cmd::Bake => {
            let cancel = AtomicBool::new(false);
            match jobs::bake_blocking(&paths, &cancel, |_| {}) {
                Ok(path) => {
                    println!("{}", path.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    tracing::error!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Build {
            sources,
            output_dir,
        } => {
            let settings = merged(&paths, sources);
            let output_dir = output_dir
                .or_else(|| settings.output_dir.clone())
                .unwrap_or_else(|| PathBuf::from("."));
            let inputs = settings.inputs(&payload);
            let cancel = AtomicBool::new(false);
            match jobs::build_blocking(&inputs, &paths, &payload, &output_dir, &cancel, |_| {}) {
                Ok(path) => {
                    println!("{}", path.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    tracing::error!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::PackProbe { output_dir } => {
            let saved = Settings::load(&paths.settings_file());
            let output_dir = output_dir
                .or(saved.output_dir)
                .unwrap_or_else(|| PathBuf::from("."));
            match psv_builder::pack::build_probe_vpk(&payload.eboot(), &output_dir) {
                Ok(path) => {
                    println!("{}", path.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    tracing::error!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Env(sources) => {
            let settings = merged(&paths, sources);
            let checks = report::run_checks(&settings.inputs(&payload));
            println!(
                "{}",
                diagnostics::build_report(&settings, &checks, &logging.buffer, &paths, &payload)
            );
            ExitCode::SUCCESS
        }
        Cmd::Prepare(sources) => {
            let inputs = merged(&paths, sources).inputs(&payload);
            let cancel = AtomicBool::new(false);
            match jobs::prepare_blocking(&inputs, &paths, &payload, &cancel, |_| {}) {
                Ok(output) => {
                    tracing::info!("game data prepared in {}", output.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    tracing::error!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
