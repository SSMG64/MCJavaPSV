# Contributing

Thanks for helping. Please read the [Code of Conduct](CODE_OF_CONDUCT.md) and the
[legal rules](#the-legal-rules) below first. They matter more here than in most projects.

## The legal rules

1. Never commit Minecraft jars, assets, shaders, data packs, or anything derived from an install.
2. Never commit decompiled or deobfuscated Minecraft source, in whole, in part, or lightly
   reworded. Study behavior, then write your own code in your own structure.
3. Keep parity checks against vanilla on your machine. Short numeric results (a seed, a hash,
   a count) are fine in an issue. Files from the game are not.
4. Do not ask for or post built VPKs or prepared data.

CI runs `scripts/check-public-tree.sh` and rejects many of these files, but you are the first
line of defense.

## Getting set up

See [BUILDING-WIN](BUILDING-WIN.md). The checks CI runs are:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash scripts/check-public-tree.sh
```

Run them before you open a pull request. The Vita crate is separate: see [BUILDING-PSV](BUILDING-PSV.md).

## Making a change

1. Open an issue first for anything bigger than a small fix, so we can agree on the approach.
2. Branch from `main`, keep the change focused, and write tests for behavior you change.
   Parsers and file formats should have tests against known good data.
3. Update the documentation when behavior, commands or build steps change.
4. Open a pull request and fill in the template.

## Style

- Format with `rustfmt` and keep clippy clean. Warnings are errors in CI.
- Avoid `unwrap` and `expect` outside tests. The workspace lints warn on them.
- `unsafe` is forbidden in the host crates.
- Comment only where the code cannot explain why. Do not restate what a line does.
- Prefer clear names and small functions over comments.
- Write documentation in plain language, and avoid em dashes.
- Keep dependencies few. A new one needs a reason, a compatible license and a reasonable MSRV
  (the workspace requires Rust 1.95).

## Performance work

The Vita is slow and small, so measure before you optimize and say how you measured: device or
emulator, build profile, and numbers. An emulator is not evidence of Vita speed.

## Updating Horizon

Change `packaging/horizon-rev.txt` and the `rev` values in `crates/psv-runtime/Cargo.toml`
together, rebuild both the Windows app and the Vita crate, and say in the pull request what
changed upstream between the two revisions.

## Commit messages

A short summary line in the imperative mood ("Add SFO parser"), then a body explaining why if it
is not obvious.

## Reporting security problems

Not here. See [SECURITY](SECURITY.md).
