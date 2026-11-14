use std::env;

/// The libraries the graphics demo links, in link order. vitaGL comes first because it uses
/// the ones after it. `PSV_GFX_LIBS` replaces the list, space separated, when an SDK differs.
const GFX_LIBS: &[&str] = &[
    "vitaGL",
    "mathneon",
    "vitashark",
    "SceShaccCgExt",
    "taihen_stub",
    "SceShaccCg_stub",
    "SceKernelDmacMgr_stub",
    "SceGxm_stub",
    "SceDisplay_stub",
    "SceCommonDialog_stub",
    "SceSysmodule_stub",
    "SceAppMgr_stub",
    "SceAppUtil_stub",
    "SceCtrl_stub",
    "SceLibKernel_stub",
    "zip",
    "z",
];

fn main() {
    println!("cargo:rerun-if-env-changed=PSV_GFX_LIBS");
    println!("cargo:rerun-if-changed=build.rs");
    let on_vita = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("vita");
    if !on_vita || env::var_os("CARGO_FEATURE_GFX").is_none() {
        return;
    }
    match env::var("PSV_GFX_LIBS") {
        Ok(list) => list
            .split_whitespace()
            .for_each(|lib| println!("cargo:rustc-link-lib={lib}")),
        Err(_) => GFX_LIBS
            .iter()
            .for_each(|lib| println!("cargo:rustc-link-lib={lib}")),
    }
}
