mod probe;

use std::path::Path;
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
    std::thread::sleep(Duration::from_secs(5));
}
