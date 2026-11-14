# Legal

This page explains what this project is, what it contains, and what you are responsible for.
**It is not legal advice.** The maintainers are not lawyers. If you need certainty about your
situation, ask one.

## What this project is not

MCJavaPSV is an independent fan project. It is not made, approved or supported by Mojang,
Microsoft, Sony, or the Horizon authors. Minecraft is a trademark of Mojang Synergies AB.
PlayStation and PS Vita are trademarks of Sony Interactive Entertainment Inc. All other names
belong to their owners and appear here only to say what the project works with.

## What this repository contains

Source code and documentation under the [MIT License](../LICENSE). It does **not** contain,
and must never contain:

- Minecraft jars, classes, textures, sounds, shaders, data packs or any other game file,
  including logos, block textures and icons, even as an application icon
- decompiled, deobfuscated or paraphrased-line-by-line Minecraft source code
- files derived from a player's install (prepared data, baked data, builds)

`scripts/check-public-tree.sh` runs in CI and rejects many such files. It is a safety net, not a
guarantee. The prebuilt `eboot.bin` in releases is built from this repository's code and its
dependencies only.

## What you must provide

You need your own legally obtained copy of Minecraft: Java Edition 26.3. The app cannot verify a
purchase. It only checks that the files of a normal install are there and unmodified. You are
responsible for holding a valid license and for following the
[Minecraft EULA](https://www.minecraft.net/en-us/eula) and Mojang's usage guidelines.

## What you build

The builder reads **your** copy of the game. A VPK built from it, including the `data.pak`
inside it, will contain material derived from your copy.

- Use it on your own console, for yourself.
- **Do not distribute it.** Do not upload it, share it, sell it, or put it in a release. Doing so
  may infringe Mojang's and Microsoft's copyrights and break the EULA, and you alone would be
  responsible.
- Do not ask for built files in issues or discussions, and do not post any.

## Horizon

Game logic in this project comes from [Horizon](https://github.com/ludovic111/horizon),
copyright Ludovic Marie and the Horizon contributors, licensed under the MIT License. Its notice
is kept in [LICENSE](../LICENSE) and shipped as `HORIZON-LICENSE` in release payloads.

Horizon's authors describe it as written from scratch and checked against vanilla. This project
relies on that description and cannot verify it. If its status or license changes, this
project will reassess.

## Contributing without copying

Contributors may study the vanilla game's behavior. They must write their own code in their
own structure and must not commit anything copied or closely transcribed from decompiled
source. Parity checks against vanilla stay on your machine. See [CONTRIBUTING](CONTRIBUTING.md).

## Third-party software

| Component | License (summary) |
| --- | --- |
| Rust crates in `Cargo.lock` | Mostly MIT or Apache-2.0. Each keeps its own license |
| Horizon | MIT |
| VitaSDK and its libraries | Several licenses, including GPL for parts of the toolchain. The Windows app does not bundle VitaSDK |
| `cargo-vita` | MIT or Apache-2.0 |
| vitaGL | LGPL v3. Only in the graphics demo build |

A generated list of third-party licenses for releases is planned and not done yet.

## vitaGL

The graphics demo build links [vitaGL](https://github.com/Rinnegatamante/vitaGL), which is under
the GNU Lesser General Public License, version 3. Linking it into this MIT project is allowed. If
you distribute the graphics build, ship vitaGL's license text and a way to get its source, and let
recipients relink with a modified vitaGL. The source here builds the executable, which covers the
last condition. The default build does not include vitaGL.

The demo needs `libshacccg.suprx` on the console. You extract it from your own console. This
project does not provide it, and whether extracting it is allowed where you live is your
responsibility.

## The Vita

Running homebrew on a Vita requires a modified console. This project does not provide, link to,
or support any exploit, firmware modification or protection circumvention tool. Modifying a
console can void warranties, break Sony's terms, and be restricted by law where you live. That
decision and its consequences are yours.

## Rights holders

If you believe something here infringes your rights, open a private report through
[GitHub security advisories](https://github.com/SSMG64/MCJavaPSV/security/advisories/new) or contact
[@SSMG64](https://github.com/SSMG64) on GitHub. The material will be reviewed and removed if appropriate.

## No warranty

The software is provided as is. See the license for the full disclaimer.
