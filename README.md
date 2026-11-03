# MCJavaPSV

An effort to run Minecraft: Java Edition 26.3 on the PlayStation Vita, written in Rust.

> **Status: Phase 0, foundation only. Nothing playable exists yet.**
> The Windows builder checks your setup and prepares your game data. The Vita
> executable is a probe. See [the roadmap](docs/ARCHITECTURE.md#roadmap) for what comes next.

## How it works

1. You install Minecraft Java 26.3 with the official launcher. You must own it.
2. The Windows builder reads **your own copy**, converts what the Vita needs, and
   writes a VPK to a folder you choose.
3. You install the VPK on your own, modified Vita.

This repository ships **no Mojang assets, code, jars, or decompiled sources**, and
the VPK you build is for your personal use only. Read [LEGAL](docs/LEGAL.md) before you use it.

## The approach

The game logic is based on [Horizon](https://github.com/ludovic111/horizon), an MIT-licensed
rewrite of Minecraft Java 26.3 in Rust that is checked against vanilla with differential
harnesses. This project adapts that logic to the Vita and adds a Vita renderer, audio,
input and a build pipeline. Whether the result can match the PC game's feel at playable
speed on Vita hardware is the open question, and it is measured, not assumed.

## Quick start (Windows)

1. Install Minecraft Java 26.3 from the official launcher and start it once.
2. Install a Java 25 JDK, for example Eclipse Temurin 25.
3. Download the 26.3 server jar from the official Minecraft website.
4. Run `mcjavapsv.exe`, pick the three paths, press **Check setup**, then **Prepare game data**.

Full steps are in [USAGE](docs/USAGE.md).

## Repository layout

| Path | What it is |
| --- | --- |
| `crates/psv-builder` | The Windows app (`mcjavapsv`) and its command line twin (`mcjavapsv-cli`) |
| `crates/psv-setup` | Finds and validates your Minecraft install and Java, and drives Horizon's preparer |
| `crates/psv-vpk` | `PARAM.SFO` and VPK writers, tested byte for byte against VitaSDK's `vita-mksfoex` |
| `crates/psv-runtime` | The executable that runs on the Vita (built separately with VitaSDK) |
| `packaging/` | The pinned Horizon revision and release packaging notes |
| `scripts/` | Repository checks, including the guard that keeps Mojang files out |
| `docs/` | All documentation |

## Documentation

Start at [docs/README.md](docs/README.md).

## License and credits

MIT. See [LICENSE](LICENSE), which keeps Horizon's copyright notice as its license requires.
Minecraft is a trademark of Mojang Synergies AB. This project is not affiliated with or
endorsed by Mojang, Microsoft, Sony, or the Horizon authors.
