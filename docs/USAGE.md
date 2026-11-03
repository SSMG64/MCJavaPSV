# Usage

This guide takes you from a fresh Windows PC to prepared game data. **Today the builder
stops after preparing your data: building the final VPK arrives in a later phase**, and the
Vita executable is still a probe. Read [LEGAL](LEGAL.md) first.

## What you need

| Item | Notes |
| --- | --- |
| Windows 10 or 11, 64-bit | Linux and macOS builds of the app exist for development but are not supported for players |
| Minecraft: Java Edition **26.3**, installed with the official launcher | You must own it. Start version 26.3 once so the launcher downloads every file |
| A Java 25 JDK | For example Eclipse Temurin 25. The Java the launcher installs is a runtime without a compiler, and the preparer compiles small helper programs |
| The official 26.3 server jar | Download it yourself from the official Minecraft website. The app checks it against the hash in your version file |
| A PlayStation Vita that can run homebrew | See [LEGAL](LEGAL.md#the-vita). This project does not provide or link to any way to modify a console |

Free disk space: not measured yet.

## Steps

1. Download a release zip and extract it somewhere you can write to. Keep the `payload` folder
   next to `mcjavapsv.exe`.
2. Start `mcjavapsv.exe`. It looks for your Minecraft folder (`%APPDATA%\.minecraft`) and fills it in.
3. Set **Server jar** with **Browse**. Leave **Java folder** empty to detect Java automatically, or
   point it at your Java 25 JDK folder.
4. Press **Check setup**. Each line is `PASS`, `WARN` or `FAIL`.
5. When nothing fails, press **Prepare game data**. Progress and messages appear in the console.
6. **Build VPK** is disabled until a later phase.

## What the checks mean

| Check | Fails when |
| --- | --- |
| Minecraft install | `versions\26.3\26.3.json` or `26.3.jar` is missing, or the jar does not match the hash in the version file |
| Server jar | Nothing selected, the file is missing, or it is not the official 26.3 jar |
| Java | No Java 25 was found. A runtime without a compiler gives `WARN`, because preparing may still work if precompiled helpers are bundled |
| Bundled preparer | `payload\horizon-prepare.exe` is missing: you moved the exe out of the release folder |

## The console

The panel at the bottom shows everything the app logs. Use the level boxes to filter, **Copy** to
copy all of it, and **Open logs folder** to see the log files. Type a command in the prompt:

| Command | Does |
| --- | --- |
| `help` | Lists commands |
| `check` | Same as **Check setup** |
| `prepare` | Same as **Prepare game data** |
| `cancel` | Stops the running job |
| `env` | Prints the detected setup and system details |
| `logs` | Prints the log folder |
| `export` | Saves a diagnostics report into the data folder |
| `version` | Prints the app version |
| `clear` | Empties the console |

## Command line

`mcjavapsv-cli` does the same without a window:

```
mcjavapsv-cli check   [--minecraft-dir DIR] [--server-jar FILE] [--java-home DIR]
mcjavapsv-cli prepare [--minecraft-dir DIR] [--server-jar FILE] [--java-home DIR]
mcjavapsv-cli env
mcjavapsv-cli logs
```

Add `--verbose` for debug messages. It exits with a non-zero code when a check or the job fails.
`RUST_LOG` overrides the log filter.

## Where files go

| What | Where (Windows) |
| --- | --- |
| Settings, logs, work files, diagnostics | `%LOCALAPPDATA%\MCJavaPSV` |
| Prepared game data | `%LOCALAPPDATA%\MCJavaPSV\work\prepared` |
| Log files | `%LOCALAPPDATA%\MCJavaPSV\logs` (the last 7 days are kept) |

Set `MCJAVAPSV_HOME` to use another folder. Deleting the folder removes everything the app stored.

## Troubleshooting

| Problem | Try |
| --- | --- |
| Java shows `WARN` or `FAIL` | Install a Java 25 JDK and press **Check setup** again, or set **Java folder** |
| Server jar hash mismatch | Download it again from the official site. It must be the 26.3 server jar |
| Hash mismatch on the Minecraft install | In the launcher, reinstall version 26.3 |
| Bundled preparer missing | Extract the whole release zip and run the exe from inside it |
| The window does not open | Update your graphics driver. The app needs OpenGL |
| Something else | Type `export`, read the file, then [open an issue](https://github.com/SSMG64/MCJavaPSV/issues) |
