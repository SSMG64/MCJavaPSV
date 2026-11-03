# Security

## Supported versions

The project is pre-release. Only the latest commit on `main` and the latest release get fixes.

## Reporting a vulnerability

Use [private vulnerability reporting](https://github.com/SSMG64/MCJavaPSV/security/advisories/new).
Please do not open a public issue for a vulnerability.

Include what you found, how to reproduce it, and what you think the impact is. Do not include
Minecraft files.

We are volunteers. We aim to acknowledge a report within 7 days and to say what we plan to do
within 30, but we cannot promise a fix date. We will credit you in the advisory unless you
prefer not to be named.

## In scope

- The builder app and CLI: path handling, file writing, the VPK and SFO writers, and starting
  child processes
- Tampering with the payload (preparer, lock file, `eboot.bin`) that the app would not notice
- The CI workflows and release process: for example secret exposure or an unpinned action
- The Vita executable in this repository

## Out of scope

- Minecraft, the launcher, Java, or Vita firmware
- Console exploits, which this project does not provide
- Bugs in Horizon itself: report those to the [Horizon](https://github.com/ludovic111/horizon) project
- Reports that need an attacker who already controls your computer

## What we do for supply chain safety

- GitHub Actions are pinned to full commit hashes and updated by Dependabot
- Dependabot also updates Cargo dependencies, and CodeQL scans the Rust code and the workflows
- Cargo builds use `--locked`, so CI builds exactly what `Cargo.lock` records
- Releases include `SHA256SUMS`. Verify your download against it

## Verifying a download

On Windows, in the extracted folder: `Get-FileHash <file> -Algorithm SHA256`, then compare with the
matching line in `SHA256SUMS`. The sums prove the files are the ones CI built, not that CI is
trustworthy. Read the workflows if that matters to you.
