# Privacy

Short version: **the app has no accounts, no telemetry and makes no network requests of its own.**
Everything it stores stays on your computer, and nothing is sent anywhere unless you send it.

This page describes the builder in this repository. We have not audited every tool it runs,
and you can confirm network silence with your firewall.

## What the app reads

- Your Minecraft folder, to check the version file and jar hashes and to give the files to
  Horizon's preparer.
- The server jar and Java folder you select.
- Environment variables used to find Java and your user folders (`JAVA_HOME`, `PATH`,
  `APPDATA`, `LOCALAPPDATA`, `HOME`).

## What the app stores

All of it lives in `%LOCALAPPDATA%\MCJavaPSV` (or `MCJAVAPSV_HOME`):

| File | Contents |
| --- | --- |
| `settings.json` | The paths you chose |
| `logs\mcjavapsv.*.log` | Messages the app and its preparer printed, including file paths. The last 7 days are kept |
| `work\` | Files produced while preparing and baking your game data (`prepared`, `baked`). These derive from your Minecraft copy |
| `diagnostics-*.txt` | Only if you run `export`. See below |

Log files are **not** redacted. Your account name appears wherever it is part of a path.
Delete the folder to remove everything.

## Diagnostics export

`export` writes a text report with the app version, your OS and CPU architecture, the paths
and results of the setup checks, and the last 300 console lines. Your home folder is replaced
with `~`. The report is never uploaded. **Read it before posting it**: GitHub issues are public
and other parts of a path may still identify you.

## What leaves your computer

Nothing, by the app. The preparer and the Java tools it starts run locally.
If you download a release, open an issue or attach a file, GitHub receives that
according to [GitHub's privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement).
The launcher and Minecraft website have their own policies.

## Children

The software is not directed at children. The Minecraft EULA sets its own age rules.

## Changes

If the app ever gains network features, this page changes first and the feature will be opt-in.
