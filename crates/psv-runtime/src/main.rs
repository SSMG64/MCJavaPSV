#[cfg_attr(not(feature = "gfx"), allow(dead_code))]
mod controls;
#[cfg(feature = "gfx")]
mod demo;
#[cfg(feature = "gfx")]
mod gfx;
mod probe;
#[cfg_attr(not(feature = "gfx"), allow(dead_code))]
mod scene;

use std::path::Path;
#[cfg(not(feature = "gfx"))]
use std::time::Duration;

const HEAP_MIB: u32 = 192;
const REPORT_DIR: &str = "ux0:data/MCJavaPSV";

#[used]
#[unsafe(export_name = "_newlib_heap_size_user")]
static NEWLIB_HEAP_SIZE_USER: u32 = HEAP_MIB * 1024 * 1024;

fn main() {
    let config = probe::Config::vita(HEAP_MIB as usize);
    let report = probe::run(&config, Path::new(REPORT_DIR));
    print!("{}", report.text());
    #[cfg(feature = "gfx")]
    demo::run();
    #[cfg(not(feature = "gfx"))]
    std::thread::sleep(Duration::from_secs(5));
}
