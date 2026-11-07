# Building the Vita executable

Players do not need this: releases ship a prebuilt `eboot.bin` made by CI, and the Windows app
only packs the VPK. Follow this guide to work on the Vita code, or to build the executable
yourself.

> **Status.** The first CI run built the probe, including Horizon's `horizon-core` and
> `horizon-world`, for the Vita target with these steps. It has not been run on a console yet,
> so the memory and speed figures are still unknown.

## Why it is different

`armv7-sony-vita-newlibeabihf` is a tier 3 Rust target: no prebuilt standard library, nightly
Rust with `-Z build-std`, VitaSDK's GCC as the linker, and only part of `std`. The Vita crate
therefore lives outside the main workspace (`crates/psv-runtime`, its own `Cargo.lock`).

## Prerequisites

1. **VitaSDK.** Use the official installer:
   ```
   git clone https://github.com/vitasdk/vdpm
   ./vdpm/bootstrap-vitasdk.sh
   export VITASDK=/usr/local/vitasdk
   export PATH="$VITASDK/bin:$PATH"
   ```
   On Windows use `bootstrap-vitasdk.ps1`, or WSL. See the [vdpm](https://github.com/vitasdk/vdpm) readme for options such as `--install-dir`.
2. **Nightly Rust with the standard library sources:**
   ```
   rustup toolchain install nightly --component rust-src
   ```
3. **cargo-vita:**
   ```
   cargo +nightly install cargo-vita --locked
   ```

## Build

```
cd crates/psv-runtime
cargo vita build eboot -- --release
```

`cargo-vita` links the ELF, runs `vita-elf-create`, then `vita-make-fself`. The result is
`target/armv7-sony-vita-newlibeabihf/release/psv-runtime.self`: that file is the eboot. It is only
renamed to `eboot.bin` when a package is built or uploaded, so copy it under that name yourself
(CI does the same). Do not set `strip` in the Cargo profile: `cargo-vita` has its own optional
strip step.

To try the probe on a console, put the `eboot.bin` in the app's `payload` folder and run
`mcjavapsv-cli pack-probe`, or build a VPK directly:

```
cargo vita build vpk -- --release
```

Install it with VitaShell, run it, and read `ux0:data/MCJavaPSV/probe.txt`. [USAGE](USAGE.md#try-the-probe-on-a-vita)
explains every line. CI also publishes the finished package as the `probe-vpk` artifact.

The probe's logic lives in `crates/psv-runtime/src/probe.rs` and runs on the host too:
`cargo test` in that folder checks the report format and the recorded noise checksum.

## Faster iteration

With `vitacompanion` installed on the Vita, `cargo vita build eboot --update --run -- --release`
uploads and starts the build over FTP, and `cargo vita logs` receives output from PrincessLog.
Set `VITA_IP` to your console's address.

## Using the result with the Windows app

Copy the `eboot.bin` into the app's `payload` folder, or pass it to `scripts/stage-payload.sh`.

## Settings worth knowing

| Setting | Where | Notes |
| --- | --- | --- |
| Heap size | `HEAP_MIB` in `src/main.rs` | 192 MiB in the probe. The probe reports how much the console really grants |
| Memory mode | `ATTRIBUTE2` in `PARAM.SFO` | `psv-vpk` defaults to `12`, as `vita-mksfoex` does |
| Nightly version | `crates/psv-runtime/rust-toolchain.toml` | Unpinned for now. Pin a date after the first green CI run |
| Horizon revision | `crates/psv-runtime/Cargo.toml` and `packaging/horizon-rev.txt` | Change both together |

## Distribution notice

A VPK built from the probe contains no Mojang data. Once later phases add data converted from
your own Minecraft install, the VPK you build contains material derived from your copy. It is
for your own console only and **you must not distribute it**. Doing so may be copyright
infringement and a breach of the Minecraft EULA, and you would be the one responsible. See [LEGAL](LEGAL.md).
