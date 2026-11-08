#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,
    Clear,
    Check,
    Prepare,
    Pack,
    Bake,
    Build,
    Cancel,
    Env,
    Logs,
    Export,
    Version,
}

pub const HELP: &str = "\
Console commands:
  help      show this list
  check     verify the Minecraft install, server jar, Java and bundled files
  prepare   run Horizon's preparer on your own game files
  bake      turn the prepared game data into the Vita data pack
  build     prepare, bake and write the full VPK into the output folder
  pack      build the probe VPK (no game data) into the output folder
  cancel    stop the running job
  env       print the detected setup and system details
  logs      show where the log files are kept
  export    write a diagnostics report you can attach to a bug report
  version   print the app version
  clear     empty this console";

pub fn parse(input: &str) -> Result<Option<Command>, String> {
    let word = input.trim().to_ascii_lowercase();
    let command = match word.as_str() {
        "" => return Ok(None),
        "help" | "?" => Command::Help,
        "clear" | "cls" => Command::Clear,
        "check" => Command::Check,
        "prepare" => Command::Prepare,
        "pack" => Command::Pack,
        "bake" => Command::Bake,
        "build" => Command::Build,
        "cancel" | "stop" => Command::Cancel,
        "env" => Command::Env,
        "logs" => Command::Logs,
        "export" => Command::Export,
        "version" => Command::Version,
        other => return Err(format!("unknown command {other:?}; type help for the list")),
    };
    Ok(Some(command))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_commands_case_insensitively() {
        assert_eq!(parse("  CHECK "), Ok(Some(Command::Check)));
        assert_eq!(parse("stop"), Ok(Some(Command::Cancel)));
        assert_eq!(parse("?"), Ok(Some(Command::Help)));
    }

    #[test]
    fn blank_is_nothing_and_unknown_is_an_error() {
        assert_eq!(parse("   "), Ok(None));
        assert!(parse("rm -rf").unwrap_err().contains("unknown command"));
    }

    #[test]
    fn every_command_is_documented() {
        for word in [
            "help", "check", "prepare", "bake", "build", "pack", "cancel", "env", "logs", "export",
            "version", "clear",
        ] {
            assert!(HELP.contains(word), "{word} missing from HELP");
        }
    }
}
