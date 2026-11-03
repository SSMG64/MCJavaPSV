# Building the Windows app

This builds `mcjavapsv.exe` and `mcjavapsv-cli.exe`. The Vita executable has its own guide:
[BUILDING-PSV](BUILDING-PSV.md).

## Prerequisites

- Rust stable **1.95 or newer** (the GUI library requires it): `rustup toolchain install stable`
- Git
- Windows 10 or 11 for the shipped build. The workspace also builds on Linux and macOS, which is what CI uses for lint and tests.

## Build and test

```
cargo build --release -p psv-builder
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check-public-tree.sh
```

The binaries are in `target/release/`. A debug build (`cargo build -p psv-builder`) opens a console
window and also logs to it, which helps when something goes wrong at startup.

## Assembling a release folder by hand

The app expects a `payload` folder next to it with Horizon's preparer and its lock file.

```
bash scripts/fetch-horizon.sh horizon-src
cargo build --release --locked --manifest-path horizon-src/Cargo.toml -p horizon-prepare
bash scripts/stage-payload.sh horizon-src target/release/payload [path/to/eboot.bin]
```

`fetch-horizon.sh` checks out the revision pinned in `packaging/horizon-rev.txt`. The `eboot.bin`
argument is optional until the pack stage exists. Without it the payload simply has none.

## Environment variables

| Variable | Effect |
| --- | --- |
| `MCJAVAPSV_HOME` | Replaces the per-user data folder (settings, logs, work files) |
| `MCJAVAPSV_PAYLOAD` | Replaces the `payload` folder location |
| `RUST_LOG` | Overrides the log filter, for example `RUST_LOG=debug` |

## What CI builds

`build.yml` builds the Windows app in **debug and release**, builds `horizon-prepare` from the
pinned Horizon revision, adds the Vita executable when that job succeeded, writes `SHA256SUMS`,
and uploads the folder as an artifact. A tag starting with `v` publishes the release build as a
GitHub release. Check `SHA256SUMS` after downloading.

## Troubleshooting

| Problem | Try |
| --- | --- |
| `rustc 1.9x or newer is required` | Update with `rustup update stable` |
| The app says the preparer is missing | Run `stage-payload.sh` as above, or set `MCJAVAPSV_PAYLOAD` |
| Linking fails on Linux | Install your distribution's X11 and Wayland development packages |
