# Architecture

This document explains how the project is put together, what is built today, and what is
still unproven.

## Goals and honest limits

The goal is a Vita build of Minecraft: Java Edition 26.3 whose rules and feel match the PC
game: the same physics, the same worldgen for a given seed, the same block and entity
behavior. The Vita has a quad-core ARM CPU at a few hundred MHz, a few hundred MB of usable
RAM and a GLES2 class GPU, so the PC game's render distance and frame rate are out of reach.
What can be held constant is behavior. What can vary is how far you see and how fast chunks
appear. Everything about speed and memory is treated as unknown until it is measured on
hardware.

## The two programs

```
 Windows                                            PlayStation Vita
 +--------------------------------------+           +---------------------------+
 | mcjavapsv.exe                        |           | MCJavaPSV (eboot.bin)     |
 |  validate  -> prepare -> bake -> pack|  VPK      |  game logic (Rust)        |
 |                                      | --------> |  renderer (GXM)           |
 | uses YOUR Minecraft install          |  install  |  reads baked data from    |
 +--------------------------------------+           |  its own data folder      |
                                                    +---------------------------+
```

Nothing from Mojang is stored in this repository or inside the prebuilt Vita executable.
All game data reaches the Vita as files the builder derives from your own install.

## Crates

| Crate | Target | Role |
| --- | --- | --- |
| `psv-vpk` | host | Writes `PARAM.SFO` and the VPK ZIP. The SFO output is identical to VitaSDK's `vita-mksfoex`, checked against files made by the real tool. ZIP output is deterministic. |
| `psv-setup` | host | Finds the Minecraft folder, reads the version file, verifies jar hashes, finds Java, and runs Horizon's `horizon-prepare` while streaming its JSON progress. |
| `psv-builder` | host (Windows first) | The `mcjavapsv` window and the `mcjavapsv-cli` tool. Owns logging, the in-app console, settings and diagnostics. |
| `psv-runtime` | Vita | The Vita executable. Built outside the main workspace with nightly Rust and VitaSDK. Today it is a probe that links Horizon's logic crates. |

## The build pipeline

| Stage | Status | What happens |
| --- | --- | --- |
| 1. Validate | working | Version 26.3 is installed, the client jar matches the hash in its version file, the server jar matches the hash in the version file, a suitable Java exists, the bundled files are present. |
| 2. Prepare | working, untested end to end | Runs `horizon-prepare` with `--prepare --progress json` against your files. It needs Java, and a full JDK unless precompiled exporters are used. Output goes to the app's work folder. |
| 3. Bake | not started | Converts the prepared data into compact Vita formats: textures, compressed audio, packed worldgen and registry tables. |
| 4. Pack | library done | `psv-vpk` writes `eboot.bin`, `sce_sys/param.sfo`, the LiveArea files and the baked data into one VPK. |

The prebuilt `eboot.bin` is produced by CI and shipped next to the app in a `payload` folder,
together with `horizon-prepare` and Horizon's `research-lock.json`. The player never needs
VitaSDK. The builder does not bundle VitaSDK itself: it is large, mostly GPL and LGPL
licensed, and only needed to compile, not to package.

## Where Horizon fits

[Horizon](https://github.com/ludovic111/horizon) is an MIT-licensed rewrite of Minecraft
Java 26.3 in Rust that its authors check against vanilla with differential harnesses.
This project pins one Horizon revision in `packaging/horizon-rev.txt` and uses it in two ways:

- **Preparer.** CI builds `horizon-prepare` from the pinned revision and ships it in the
  payload. It turns your own game files into the data Horizon reads.
- **Logic.** `horizon-core` (only depends on `libm`) and `horizon-world` (depends on
  pure Rust crates and contains no OS specific code) are the candidates to run on the Vita.
  The probe in `psv-runtime` links them. The first CI run built that probe for the Vita target, so
the two crates compile there. Whether they run correctly, and how fast, is untested on hardware.

Horizon's desktop client (`horizon-client`, `horizon-client-render`) is built on wgpu and
winit, which have no Vita backend, so it cannot run on the Vita as is. The Vita needs its
own renderer on GXM (through vitaGL or directly), audio, input and a game loop, plus a
decision about which of Horizon's features to carry over. Dropping desktop only
features such as mods, shader packs, telemetry and the narrator is expected.

## Rust on the Vita

The Vita target `armv7-sony-vita-newlibeabihf` is a tier 3 Rust target. It has no
prebuilt standard library, needs a nightly compiler with `-Z build-std`, links with VitaSDK's
GCC, and supports only part of `std`. `cargo-vita` wraps the steps: build, `vita-elf-create`,
`vita-make-fself`, then package. See [BUILDING-PSV](BUILDING-PSV.md).

`PARAM.SFO` defaults to the same memory mode flag `vita-mksfoex` uses (`ATTRIBUTE2=12`).
Whether the game needs it, and what heap size the runtime should request, is a
measurement task for the first hardware tests.

## Verifying that it plays like Minecraft

The vanilla game is the oracle. The plan is to compare traces from vanilla with traces
from the port, locally, without committing anything derived from Mojang's files:

- Seeded terrain: generate the same chunk in both and compare block hashes.
- Player physics: record a jump and sprint path in vanilla and compare it tick by tick.
- Random number streams: compare the first values for a set of seeds.

Horizon already has such harnesses for the desktop build. The Vita port reuses its logic,
so the first job is proving the Vita build produces the same numbers as the desktop build.

## Logging and diagnostics

The builder writes a daily rotating log (7 files kept) to the per-user data folder, mirrors
every message into the console panel of the window, and can export a diagnostics report with
the home folder replaced by `~`. `RUST_LOG` overrides the log filter. See [PRIVACY](PRIVACY.md).

## CI

| Workflow | Purpose |
| --- | --- |
| `lint.yml` | rustfmt, clippy with warnings denied, tests, actionlint, and the public tree guard |
| `codeql.yml` | CodeQL for Rust and for the workflows themselves |
| `build.yml` | Builds the Vita executable, builds the Windows app in debug and release, assembles the payload and publishes artifacts |
| `dependabot.yml` | Weekly updates for Cargo and GitHub Actions |

## Roadmap

Phases are tentative and each ends with something that can be checked.

| Phase | Outcome | Status |
| --- | --- | --- |
| 0 | Repository, CI, builder with checks, console and preparer driver, VPK writer, Vita probe | in this change |
| 1 | CI builds the Vita probe with Horizon's logic and it runs on hardware; first memory and speed numbers | next |
| 2 | Bake stage and a data format sized for the Vita | planned |
| 3 | Vita renderer: textures, chunk meshes, a flying camera | planned |
| 4 | World: chunk load and generation, player physics, parity traces against vanilla | planned |
| 5 | Gameplay: items, crafting, mobs, audio, input and menus | planned |
| 6 | Multiplayer over the network, if memory and CPU allow | investigate |

## Decisions still open

- Whether the Vita runtime depends on Horizon through a pinned git dependency (today) or a
  maintained fork with Vita specific changes.
- How much of Horizon's gameplay code in `horizon-client` can be separated from winit.
- The baked data format, and how aggressively textures and audio can be compressed
  while staying visually identical.
- Whether a JDK can be avoided entirely by shipping precompiled exporters, so the Java
  runtime the official launcher already installed is enough.
