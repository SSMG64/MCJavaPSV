# Usage

This guide covers what the builder can do today: check your setup, prepare your game data, and
build a **probe package** to try the Vita executable on a console. **The Vita executable inside the
VPK is still only a probe that measures the hardware, so the package is not a playable game
yet.** Read [LEGAL](LEGAL.md) first.

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
6. Press **Build VPK** (set **Output folder** first) to prepare, bake and package in one go.
   **Bake** alone converts game data you already prepared. The package holds data derived from
   your copy of the game, so do not share it. To try the Vita executable without game data, see
   [Try the probe on a Vita](#try-the-probe-on-a-vita).

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
| `bake` | Same as **Bake** |
| `build` | Same as **Build VPK** |
| `pack` | Same as **Build probe VPK** |
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
mcjavapsv-cli bake
mcjavapsv-cli build   [--minecraft-dir DIR] [--server-jar FILE] [--java-home DIR] [--output-dir DIR]
mcjavapsv-cli pack-probe [--output-dir DIR]
mcjavapsv-cli env
mcjavapsv-cli logs
```

Add `--verbose` for debug messages. It exits with a non-zero code when a check or the job fails.
`RUST_LOG` overrides the log filter.

## Try the probe on a Vita

The probe package contains the Vita executable and generic artwork, and **no Minecraft data**, so
you can build it without any game files. The executable measures what the console can do: usable
memory, speed, threads, storage speed, and whether floating point maths matches the desktop.

1. Get the package, either way:
   - download the `probe-vpk` artifact from a successful run of the **Build** workflow on GitHub, or
   - press **Build probe VPK** in the app (set **Output folder** first), or run `mcjavapsv-cli pack-probe`.
2. Copy `MCJavaPSV-probe.vpk` to your Vita and install it with VitaShell.
3. Start **MCJavaPSV Probe**. The screen stays blank because nothing draws yet. Give it a minute: it
   closes by itself when it finishes.
4. Open `ux0:data/MCJavaPSV/probe.txt` in VitaShell (or fetch it over FTP) and read it.

The report is written after every stage, so if the app hangs or crashes, the file still shows how far
it got. Close a hung app with the PS button and keep the file.

| Line | Meaning |
| --- | --- |
| `check java_random_seed_0: ok` | Horizon's Java `Random` gives Java's known first value |
| `check noise_matches_desktop: ok` | 4096 noise samples hash to the value measured on a PC. `FAILED` means ARM maths differs from x86 and needs investigating |
| `check f32_denormals_preserved: ok` | Tiny floating point values are not flushed to zero, as Java requires |
| `heap allocated and written MiB` | How much memory the game could really use. Compare with `heap configured MiB` |
| `legacy random`, `xoroshiro`, `perlin get3` | Speed of the building blocks of worldgen, in millions per second |
| `thread scaling` | How close three threads come to three times the speed of one |
| `storage write` and `storage read` | Memory card speed in MiB per second |
| `check pack_round_trip: ok` | The Vita packs and unpacks 1,024 synthetic textures and 1,024 JSON files without losing a byte |
| `pack read+decode` | How fast the Vita unpacks data: MiB per second, and million texture pixels per second |
| `data.pak: ... entries` | Only in a full package: the real data pack opens, and 500 of its textures decode |

If the app closes at once and no `probe.txt` appears, the Vita probably could not grant the memory
the probe asks for. Say so when you report it.

## Where files go

| What | Where (Windows) |
| --- | --- |
| Settings, logs, work files, diagnostics | `%LOCALAPPDATA%\MCJavaPSV` |
| Prepared game data | `%LOCALAPPDATA%\MCJavaPSV\work\prepared` |
| Baked data pack | `%LOCALAPPDATA%\MCJavaPSV\work\baked\data.pak` |
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
