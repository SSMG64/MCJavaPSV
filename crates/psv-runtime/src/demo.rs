//! The graphics demo: fly over generated terrain. It writes a trace to the memory card after
//! every step, so a crash still shows how far it got, and reports frame timing.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::time::Instant;

use psv_pack::PackReader;
use psv_render::camera::Camera;
use psv_render::gpu::{Backend, Renderer};

use crate::controls;
use crate::gfx::{self, VitaGl};
use crate::scene::{self, Scene};

const TRACE_DIR: &str = "ux0:data/MCJavaPSV";
const DATA_PACK: &str = "app0:data/data.pak";
const SCREEN: (i32, i32) = (960, 544);
const SKY: [u8; 4] = [120, 170, 255, 255];
const REPORT_EVERY_FRAMES: u32 = 120;

struct Trace(Option<File>);

impl Trace {
    fn open() -> Self {
        let _ = fs::create_dir_all(TRACE_DIR);
        Self(
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(format!("{TRACE_DIR}/gfx.txt"))
                .ok(),
        )
    }

    fn line(&mut self, text: impl std::fmt::Display) {
        println!("{text}");
        if let Some(file) = &mut self.0 {
            let _ = writeln!(file, "{text}");
            let _ = file.flush();
        }
    }
}

fn load_scene(trace: &mut Trace) -> Option<Scene> {
    if let Ok(file) = File::open(DATA_PACK) {
        match PackReader::open(file)
            .map_err(|e| e.to_string())
            .and_then(|mut p| scene::build(&mut p).map_err(|e| e.to_string()))
        {
            Ok(scene) => {
                trace.line("world: built from data.pak");
                return Some(scene);
            }
            Err(error) => trace.line(format!(
                "world: data.pak is unusable ({error}), using placeholder art"
            )),
        }
    } else {
        trace.line("world: no data.pak, using placeholder art");
    }
    match scene::placeholder_pack()
        .map_err(|e| e.to_string())
        .and_then(|mut p| scene::build(&mut p).map_err(|e| e.to_string()))
    {
        Ok(scene) => Some(scene),
        Err(error) => {
            trace.line(format!(
                "world: FAILED to build the placeholder world: {error}"
            ));
            None
        }
    }
}

pub fn run() -> ! {
    let mut trace = Trace::open();
    trace.line("gfx demo: start");

    let started = Instant::now();
    let Some(scene) = load_scene(&mut trace) else {
        gfx::exit();
    };
    trace.line(format!(
        "world: {} textures, {} sections, {} quads, built in {} ms",
        scene.stats.textures,
        scene.stats.sections,
        scene.stats.quads,
        started.elapsed().as_millis()
    ));
    trace.line(format!(
        "atlas: {}x{} ({} KiB)",
        scene.atlas.width,
        scene.atlas.height,
        scene.atlas.rgba.len() / 1024
    ));

    trace.line("vitaGL: starting. If the app closes here, libshacccg.suprx is probably missing");
    let mut renderer = Renderer::new(VitaGl::new(SCREEN.0, SCREEN.1));
    gfx::init_controller();
    trace.line("vitaGL: started");

    if renderer.backend_mut().upload_atlas(&scene.atlas).is_err() {
        trace.line("atlas upload FAILED");
        gfx::exit();
    }
    trace.line("atlas: uploaded");
    let uploading = Instant::now();
    for section in &scene.sections {
        if renderer
            .set_section(section.key, section.origin, [16.0; 3], &section.mesh)
            .is_err()
        {
            trace.line(format!("mesh upload FAILED at section {:?}", section.key));
            gfx::exit();
        }
    }
    trace.line(format!(
        "meshes: {} uploaded in {} ms",
        scene.sections.len(),
        uploading.elapsed().as_millis()
    ));

    let mut camera = Camera::new(scene.spawn, SCREEN.0 as f32 / SCREEN.1 as f32);
    camera.pitch = 20.0;
    let mut last = Instant::now();
    let (mut frames, mut window_start) = (0_u32, Instant::now());
    loop {
        let pad = gfx::read_pad();
        if controls::wants_exit(&pad) {
            trace.line("exit requested");
            gfx::exit();
        }
        let now = Instant::now();
        let seconds = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        controls::update(&mut camera, &pad, seconds);

        match renderer.draw_frame(&camera, SKY) {
            Ok(stats) => {
                frames += 1;
                if frames == 1 {
                    trace.line(format!(
                        "first frame: {} of {} sections drawn, {} draw calls",
                        stats.sections_drawn, stats.sections, stats.draw_calls
                    ));
                }
            }
            Err(error) => {
                trace.line(format!("frame FAILED: {error:?}"));
                gfx::exit();
            }
        }
        if frames > 0 && frames % REPORT_EVERY_FRAMES == 0 {
            let ms = window_start.elapsed().as_secs_f64() * 1000.0 / f64::from(REPORT_EVERY_FRAMES);
            trace.line(format!(
                "frame {frames}: {ms:.1} ms average ({:.1} fps), camera at {:.0} {:.0} {:.0}",
                1000.0 / ms,
                camera.position[0],
                camera.position[1],
                camera.position[2]
            ));
            window_start = Instant::now();
        }
    }
}
