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
| `psv-vpk` | host | Writes `PARAM.SFO` and the VPK ZIP, and generates the project's own icon and generic LiveArea art. The SFO output is identical to VitaSDK's `vita-mksfoex`, checked against files made by the real tool. ZIP output is deterministic. |
| `psv-pack` | host and Vita | The data container and the lossless texture format, shared by the baker and the Vita runtime so both read one definition. LZ4 through `lz4_flex`, the same crate Horizon uses. |
| `psv-bake` | host | Reads a prepared folder (or a jar) and writes the data pack: PNGs to texture blobs, unusable files skipped, everything else stored or compressed. |
| `psv-render` | host and Vita | The renderer without the GPU: texture atlas, block model and blockstate parsing, quad baking, chunk meshing, the camera, a `Backend` trait with a scene `Renderer` above it, and a software rasterizer used by the tests. |
| `psv-setup` | host | Finds the Minecraft folder, reads the version file, verifies jar hashes, finds Java, and runs Horizon's `horizon-prepare` while streaming its JSON progress. |
| `psv-builder` | host (Windows first) | The `mcjavapsv` window and the `mcjavapsv-cli` tool. Owns logging, the in-app console, settings and diagnostics. |
| `psv-runtime` | Vita | The Vita executable. Built outside the main workspace with nightly Rust and VitaSDK. It runs the hardware probe, and with the `gfx` feature a flying camera demo drawn through vitaGL. |

## The build pipeline

| Stage | Status | What happens |
| --- | --- | --- |
| 1. Validate | working | Version 26.3 is installed, the client jar matches the hash in its version file, the server jar matches the hash in the version file, a suitable Java exists, the bundled files are present. |
| 2. Prepare | working, untested end to end | Runs `horizon-prepare` with `--prepare --progress json` against your files. It needs Java, and a full JDK unless precompiled exporters are used. Output goes to the app's work folder. |
| 3. Bake | working | `psv-bake` turns the prepared folder into `data.pak`: every PNG becomes a lossless texture blob, files the Vita cannot use (shaders, Java) are skipped, the rest is stored or LZ4 compressed. Verified on the 26.3 assets, not yet on a real prepared folder. |
| 4. Pack | working | `build` writes `eboot.bin`, `sce_sys/param.sfo`, generic LiveArea art and `data/data.pak` into one VPK. `pack-probe` still builds a package without game data. The Vita executable does not read the data pack yet. |

The prebuilt `eboot.bin` is produced by CI and shipped next to the app in a `payload` folder,
together with `horizon-prepare` and Horizon's `research-lock.json`. The player never needs
VitaSDK. The builder does not bundle VitaSDK itself: it is large, mostly GPL and LGPL
licensed, and only needed to compile, not to package.

## The baked data format

Everything the Vita reads from the game's data lives in one file, `data/data.pak` in the VPK
(`app0:data/data.pak` on the console). The definition is in `crates/psv-pack`.

**Pack.** Little endian. A 32 byte header (`MCPK`, version, flags, entry count, index offset,
index size, index CRC, header CRC) is followed by the entry data and, at the end, a sorted
index. The index holds, per entry: path, codec (stored or LZ4), raw size, stored size, offset and
the CRC-32 of the raw bytes. A reader loads only the header and index, finds entries by binary
search, and checks each entry's CRC when it reads it. The index at the end lets the baker stream
entries without knowing their sizes. A reader refuses a pack with a newer version, a bad CRC, an
unsorted index, or any entry that points outside the file.

**Texture.** A baked PNG is `MCTX`, version, kind, flags, width and height, then either raw RGBA
rows or a palette of up to 256 colors plus one index byte per pixel. The encoder takes whichever
is smaller and never loses information. Two flags tell the renderer whether the texture has fully
transparent or partly transparent pixels. The Vita unpacks a texture with a copy or a table
lookup, without the inflate step a PNG needs.

**What is baked.** Every PNG becomes a texture at the same path with the extension `.tex`.
Shaders (`.fsh`, `.vsh`, `.glsl`), Java files and archive metadata are skipped. Sounds, NBT and zip
files are stored as they are, since they are already compressed. Everything else is LZ4
compressed when that saves at least a sixteenth.

**Measured on the 26.3 assets and data**, taken from an unpacked copy of the game, with sounds
and Horizon's own prepared files not included:

| Part | Files | Raw | In the pack |
| --- | --- | --- | --- |
| Textures | 3,965 | 13.6 MiB | 3.6 MiB (the original PNGs total 2.1 MiB) |
| JSON | 15,700 | 9.0 MiB | 3.4 MiB |
| NBT structures | 1,511 | 4.0 MiB | 4.0 MiB |
| Whole pack | 21,401 | 26.7 MiB | 12.6 MiB |

The bake took 1.8 seconds on a PC. All 3,965 textures decode to exactly the pixels of their PNG,
checked with Rust's `png` crate over every texture, and with a second reader written in Python
from this description and compared with Pillow on 400 random textures. Baked textures take more
room than PNGs, which is the price of skipping the inflate step on a slow CPU. The real prepared
folder will also hold sounds, fonts and Horizon's prepared files, so the real pack will be bigger.

## The renderer

`psv-render` turns data into things a GPU can draw, and does none of the drawing. The same code
runs on a PC in the tests and will run on the Vita.

1. **Atlas.** Baked textures are packed into one image with a pixel of edge padding, in the
   smallest power of two size, preferring a squarer shape. A strip of animation frames uses its
   first frame for now.
2. **Models.** Block model JSON is parsed, parents are followed, and texture slots are resolved,
   including the 26.x object form that can force a texture to be drawn translucent.
3. **Blockstates.** Variants, weighted lists and multipart conditions choose which models a block
   with given properties uses, with the game's turn conventions: x then y, each the negative way.
4. **Quads.** Each model face becomes a quad with corners, atlas coordinates, a shade direction, a
   cull face and a layer (solid, cutout or translucent). Element rotation with rescale, texture
   rotation and `uvlock` follow the game's rules. `uvlock` is checked by an invariant: a cube
   turned any of 16 ways with uvlock carries the same texture coordinates on the same corners.
5. **Meshing.** A region of blocks becomes vertex and index buffers per layer. Faces hidden by a
   neighbour that fully covers them are dropped, and so are the faces between two identical
   translucent blocks. Each face is lit by its direction (top 1.0, sides 0.8 and 0.6, bottom 0.5)
   and by a tint. Batches stop below 65,536 vertices because the GPU takes 16 bit indices.
6. **Camera.** First person angles with the game's compass, OpenGL style matrices, flying movement
   and a view frustum test.

Verified on the 26.3 data, taken from an unpacked copy of the game: all 1,288 blockstates and all
2,603 models they reference load, and all 10,290 block variants bake fully textured into 106,081
quads. 4,106 variants use `uvlock`. The 1,300 block textures fit one 512 by 1024 atlas of 2.0 MiB.
Checking against real data found two things the first version missed: texture slots can be
objects, and a face may name a slot without a `#`.

The tests use small textures and models written for them, and a CPU rasterizer checks what a mesh
looks like: each side of a block from outside with its own shade, textures upright and unmirrored on
every side, rotation, culling, tint, cutout and blending, and clipping at the near plane.

**Not done yet:** the GPU layer, lighting and ambient occlusion, animated textures, biome colors,
and models drawn by special code (chests, signs, items).

### The GPU layer

The renderer reaches a GPU through the `Backend` trait in `psv-render::gpu`: upload an atlas,
upload a mesh, begin a frame, draw one layer of a mesh with a matrix, end the frame. A `Renderer`
sits above it and decides what to draw. It keeps the uploaded sections, skips those outside the
view frustum, draws the solid layer, then cutout, then translucent sections from the farthest to
the nearest. A `SoftwareBackend` runs the same code on a CPU, so the flying camera loop is tested
without a GPU.

**Decision: vitaGL.** It was chosen for feasibility, not for speed.

| Route | For | Against |
| --- | --- | --- |
| vitaGL (chosen) | OpenGL style API with a fixed function pipeline, so no shaders to write at first. Widely used by Vita ports. | LGPL v3. Needs `libshacccg.suprx` on the console. Not in the SDK release, so CI installs it with `vdpm`. |
| Raw GXM | No extra library, and the most control. | The GPU shaders must be compiled by a Sony tool that is not available, or on the console by the same `libshacccg.suprx` module vitaGL needs. So it would not remove that requirement, and it adds a lot of GPU setup code to write without hardware to test on. |
| vita2d | Simple. | 2D only. |

Raw GXM has a real performance ceiling advantage, but it is unmeasured here. The `Backend` seam
keeps it possible: measure on hardware first, and if vitaGL's CPU overhead turns out to be the
bottleneck, a second backend can replace it without touching the renderer.

vitaGL's license is the Lesser GPL, version 3. Linking it into an MIT project is allowed. The
conditions are to ship its license text and a way to get its source, and to let a user relink the
program with a modified library, which the open source build here already allows. See
[LEGAL](LEGAL.md#vitagl). The user supplies `libshacccg.suprx` from their own console. This
project cannot ship it.

**What exists.** In `crates/psv-runtime`, behind the `gfx` feature: the vitaGL backend, a demo
loop, and the controls. The world is terrain from Horizon's own Perlin noise, built from
`data.pak` when there is one and from generated placeholder art when there is not, so the
graphics can be tried with no game files. The demo writes `gfx.txt` to the memory card after every
step, with frame timing, so a crash still shows how far it got.

**What is verified.** The controls, the scene, the mesh packing and everything above the backend
are tested on a PC, and the backend type-checks and passes clippy. On real 26.3 data the scene
builds with 7 textures, a 64 by 64 atlas and about 18,000 quads. **Never linked or run on a Vita.**
The list of libraries it links is a best guess taken from vitaGL's own samples.

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
| 1 | CI builds the Vita probe with Horizon's logic and it runs on hardware; first memory and speed numbers | in progress: CI builds it and packages a probe VPK, waiting for hardware results |
| 2 | Bake stage and a data format sized for the Vita | done: `bake` and `build` work and the format is verified. Not yet run on a real prepared folder |
| 3 | Vita renderer: textures, chunk meshes, a flying camera | 3a done and verified on a PC. 3b, the vitaGL backend and a flying camera demo, is written and type-checked but has never been linked or run on a Vita |
| 4 | World: chunk load and generation, player physics, parity traces against vanilla | planned |
| 5 | Gameplay: items, crafting, mobs, audio, input and menus | planned |
| 6 | Multiplayer over the network, if memory and CPU allow | investigate |

## Decisions still open

- Whether the Vita runtime depends on Horizon through a pinned git dependency (today) or a
  maintained fork with Vita specific changes.
- How much of Horizon's gameplay code in `horizon-client` can be separated from winit.
- Whether sounds should stay as Ogg Vorbis or be decoded or re-encoded for the Vita's CPU. That
  needs a decoder measured on hardware.
- Whether to pre-stitch texture atlases at bake time or build them when the game starts.
- Whether a JDK can be avoided entirely by shipping precompiled exporters, so the Java
  runtime the official launcher already installed is enough.
