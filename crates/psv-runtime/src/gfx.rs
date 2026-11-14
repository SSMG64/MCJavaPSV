//! The vitaGL backend: puts the atlas and chunk meshes on the Vita's GPU and draws them.
//!
//! It uses vitaGL's fixed function pipeline, which compiles its own shaders on first use through
//! the console's `libshacccg.suprx`. That module has to be on the console already.

use std::ffi::c_void;

use psv_render::atlas::Atlas;
use psv_render::gpu::Backend;
use psv_render::math::Mat4;
use psv_render::mesh::Mesh;
use psv_render::quad::Layer;

use crate::controls::Pad;

type GLenum = u32;
type GLuint = u32;
type GLint = i32;
type GLsizei = i32;
type GLbitfield = u32;
type GLboolean = u8;
type GLfloat = f32;
type GLsizeiptr = isize;

const GL_FALSE: GLboolean = 0;
const GL_TRUE: GLboolean = 1;
const GL_DEPTH_BUFFER_BIT: GLbitfield = 0x0100;
const GL_COLOR_BUFFER_BIT: GLbitfield = 0x4000;
const GL_BACK: GLenum = 0x0405;
const GL_CCW: GLenum = 0x0901;
const GL_CULL_FACE: GLenum = 0x0B44;
const GL_DEPTH_TEST: GLenum = 0x0B71;
const GL_ALPHA_TEST: GLenum = 0x0BC0;
const GL_BLEND: GLenum = 0x0BE2;
const GL_TEXTURE_2D: GLenum = 0x0DE1;
const GL_GREATER: GLenum = 0x0204;
const GL_LEQUAL: GLenum = 0x0203;
const GL_SRC_ALPHA: GLenum = 0x0302;
const GL_ONE_MINUS_SRC_ALPHA: GLenum = 0x0303;
const GL_UNSIGNED_BYTE: GLenum = 0x1401;
const GL_UNSIGNED_SHORT: GLenum = 0x1403;
const GL_FLOAT: GLenum = 0x1406;
const GL_RGBA: GLenum = 0x1908;
const GL_TRIANGLES: GLenum = 0x0004;
const GL_NEAREST: GLint = 0x2600;
const GL_TEXTURE_MAG_FILTER: GLenum = 0x2800;
const GL_TEXTURE_MIN_FILTER: GLenum = 0x2801;
const GL_TEXTURE_WRAP_S: GLenum = 0x2802;
const GL_TEXTURE_WRAP_T: GLenum = 0x2803;
const GL_CLAMP_TO_EDGE: GLint = 0x812F;
const GL_MODELVIEW: GLenum = 0x1700;
const GL_PROJECTION: GLenum = 0x1701;
const GL_VERTEX_ARRAY: GLenum = 0x8074;
const GL_COLOR_ARRAY: GLenum = 0x8076;
const GL_TEXTURE_COORD_ARRAY: GLenum = 0x8078;
const GL_ARRAY_BUFFER: GLenum = 0x8892;
const GL_ELEMENT_ARRAY_BUFFER: GLenum = 0x8893;
const GL_STATIC_DRAW: GLenum = 0x88E4;

const VERTEX_BYTES: usize = 24;
const ALPHA_CUTOFF: GLfloat = 0.1;

unsafe extern "C" {
    fn vglInitExtended(gpu_pool_size: u32, width: i32, height: i32, ram_threshold: i32, msaa: u32);
    fn vglUseVram(usage: GLboolean);
    fn vglWaitVblankStart(enable: GLboolean);
    fn vglSwapBuffers(has_commondialog: GLboolean);

    fn glViewport(x: GLint, y: GLint, width: GLsizei, height: GLsizei);
    fn glClearColor(r: GLfloat, g: GLfloat, b: GLfloat, a: GLfloat);
    fn glClear(mask: GLbitfield);
    fn glEnable(cap: GLenum);
    fn glDisable(cap: GLenum);
    fn glDepthFunc(func: GLenum);
    fn glDepthMask(flag: GLboolean);
    fn glCullFace(mode: GLenum);
    fn glFrontFace(mode: GLenum);
    fn glBlendFunc(source: GLenum, destination: GLenum);
    fn glAlphaFunc(func: GLenum, reference: GLfloat);
    fn glMatrixMode(mode: GLenum);
    fn glLoadIdentity();
    fn glLoadMatrixf(matrix: *const GLfloat);
    fn glGenTextures(count: GLsizei, textures: *mut GLuint);
    fn glBindTexture(target: GLenum, texture: GLuint);
    fn glTexParameteri(target: GLenum, name: GLenum, value: GLint);
    fn glTexImage2D(
        target: GLenum,
        level: GLint,
        internal_format: GLint,
        width: GLsizei,
        height: GLsizei,
        border: GLint,
        format: GLenum,
        kind: GLenum,
        pixels: *const c_void,
    );
    fn glGenBuffers(count: GLsizei, buffers: *mut GLuint);
    fn glBindBuffer(target: GLenum, buffer: GLuint);
    fn glBufferData(target: GLenum, size: GLsizeiptr, data: *const c_void, usage: GLenum);
    fn glDeleteBuffers(count: GLsizei, buffers: *const GLuint);
    fn glEnableClientState(array: GLenum);
    fn glVertexPointer(size: GLint, kind: GLenum, stride: GLsizei, pointer: *const c_void);
    fn glTexCoordPointer(size: GLint, kind: GLenum, stride: GLsizei, pointer: *const c_void);
    fn glColorPointer(size: GLint, kind: GLenum, stride: GLsizei, pointer: *const c_void);
    fn glDrawElements(mode: GLenum, count: GLsizei, kind: GLenum, indices: *const c_void);

    fn sceCtrlSetSamplingMode(mode: i32) -> i32;
    fn sceCtrlPeekBufferPositive(port: i32, data: *mut CtrlData, count: i32) -> i32;
    fn sceKernelExitProcess(status: i32) -> i32;
}

/// The console's controller record. The padding is generous so a longer record cannot overrun.
#[repr(C)]
struct CtrlData {
    time_stamp: u64,
    buttons: u32,
    lx: u8,
    ly: u8,
    rx: u8,
    ry: u8,
    reserved: [u8; 48],
}

const SAMPLING_ANALOG: i32 = 1;

pub fn init_controller() {
    // SAFETY: a plain call into the console's controller library with a valid mode.
    unsafe {
        sceCtrlSetSamplingMode(SAMPLING_ANALOG);
    }
}

pub fn read_pad() -> Pad {
    let mut data = CtrlData {
        time_stamp: 0,
        buttons: 0,
        lx: 128,
        ly: 128,
        rx: 128,
        ry: 128,
        reserved: [0; 48],
    };
    // SAFETY: `data` is at least as large as the console's record, and one sample is requested.
    let read = unsafe { sceCtrlPeekBufferPositive(0, &raw mut data, 1) };
    if read <= 0 {
        return Pad::REST;
    }
    Pad {
        buttons: data.buttons,
        lx: data.lx,
        ly: data.ly,
        rx: data.rx,
        ry: data.ry,
    }
}

pub fn exit() -> ! {
    // SAFETY: ends the process; nothing runs afterwards.
    unsafe {
        sceKernelExitProcess(0);
    }
    loop {
        std::thread::park();
    }
}

#[derive(Debug)]
pub enum GfxError {
    NoAtlas,
}

#[derive(Clone, Copy)]
struct DrawRange {
    vertex_offset: usize,
    index_offset: usize,
    index_count: i32,
}

/// A chunk mesh on the GPU: one vertex buffer and one index buffer, with the batches of each
/// layer recorded as ranges into them.
pub struct GpuMesh {
    buffers: [GLuint; 2],
    layers: [Vec<DrawRange>; 3],
}

impl Drop for GpuMesh {
    fn drop(&mut self) {
        // SAFETY: the buffers were created by `upload_mesh` and are deleted once.
        unsafe { glDeleteBuffers(2, self.buffers.as_ptr()) };
    }
}

pub struct VitaGl {
    texture: GLuint,
    has_atlas: bool,
}

fn offset_pointer(offset: usize) -> *const c_void {
    std::ptr::null::<u8>().wrapping_add(offset).cast()
}

impl VitaGl {
    /// Starts vitaGL at the screen's size. This is where a missing `libshacccg.suprx` shows up.
    pub fn new(width: i32, height: i32) -> Self {
        // SAFETY: documented initialisation calls, made once before any other GL call.
        unsafe {
            vglUseVram(GL_TRUE);
            vglInitExtended(0, width, height, 0x0100_0000, 0);
            vglWaitVblankStart(GL_TRUE);
            glViewport(0, 0, width, height);
        }
        Self {
            texture: 0,
            has_atlas: false,
        }
    }
}

fn pack_mesh(mesh: &Mesh) -> (Vec<u8>, Vec<u8>, [Vec<DrawRange>; 3]) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut layers: [Vec<DrawRange>; 3] = Default::default();
    for layer in Layer::ALL {
        for batch in mesh.layer(layer) {
            layers[layer.index()].push(DrawRange {
                vertex_offset: vertices.len(),
                index_offset: indices.len(),
                index_count: batch.indices.len() as i32,
            });
            for v in &batch.vertices {
                for value in v.pos.iter().chain(&v.uv) {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
                vertices.extend_from_slice(&v.color);
            }
            for index in &batch.indices {
                indices.extend_from_slice(&index.to_le_bytes());
            }
        }
    }
    (vertices, indices, layers)
}

impl Backend for VitaGl {
    type Error = GfxError;
    type Mesh = GpuMesh;

    fn upload_atlas(&mut self, atlas: &Atlas) -> Result<(), GfxError> {
        // SAFETY: `atlas.rgba` holds width * height RGBA pixels, which is what is described.
        unsafe {
            glGenTextures(1, &raw mut self.texture);
            glBindTexture(GL_TEXTURE_2D, self.texture);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as GLint,
                atlas.width as GLsizei,
                atlas.height as GLsizei,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                atlas.rgba.as_ptr().cast(),
            );
        }
        self.has_atlas = true;
        Ok(())
    }

    fn upload_mesh(&mut self, mesh: &Mesh) -> Result<GpuMesh, GfxError> {
        let (vertices, indices, layers) = pack_mesh(mesh);
        let mut buffers = [0 as GLuint; 2];
        // SAFETY: the byte vectors outlive the calls, and the sizes passed are their lengths.
        unsafe {
            glGenBuffers(2, buffers.as_mut_ptr());
            glBindBuffer(GL_ARRAY_BUFFER, buffers[0]);
            glBufferData(
                GL_ARRAY_BUFFER,
                vertices.len() as GLsizeiptr,
                vertices.as_ptr().cast(),
                GL_STATIC_DRAW,
            );
            glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, buffers[1]);
            glBufferData(
                GL_ELEMENT_ARRAY_BUFFER,
                indices.len() as GLsizeiptr,
                indices.as_ptr().cast(),
                GL_STATIC_DRAW,
            );
            glBindBuffer(GL_ARRAY_BUFFER, 0);
            glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, 0);
        }
        Ok(GpuMesh { buffers, layers })
    }

    fn begin_frame(&mut self, sky: [u8; 4]) {
        let channel = |v: u8| f32::from(v) / 255.0;
        // SAFETY: plain state calls; the depth mask is restored first so the clear reaches it.
        unsafe {
            glDepthMask(GL_TRUE);
            glClearColor(
                channel(sky[0]),
                channel(sky[1]),
                channel(sky[2]),
                channel(sky[3]),
            );
            glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
            glEnable(GL_DEPTH_TEST);
            glDepthFunc(GL_LEQUAL);
            glEnable(GL_CULL_FACE);
            glCullFace(GL_BACK);
            glFrontFace(GL_CCW);
            glEnable(GL_TEXTURE_2D);
            glBindTexture(GL_TEXTURE_2D, self.texture);
            glMatrixMode(GL_PROJECTION);
            glLoadIdentity();
            glMatrixMode(GL_MODELVIEW);
            glEnableClientState(GL_VERTEX_ARRAY);
            glEnableClientState(GL_TEXTURE_COORD_ARRAY);
            glEnableClientState(GL_COLOR_ARRAY);
        }
    }

    fn draw(&mut self, mesh: &GpuMesh, layer: Layer, mvp: &Mat4) {
        let ranges = &mesh.layers[layer.index()];
        if ranges.is_empty() {
            return;
        }
        // SAFETY: the buffers belong to `mesh`; the offsets and counts were recorded when the
        // data was uploaded, and the matrix pointer is to sixteen floats.
        unsafe {
            match layer {
                Layer::Solid => {
                    glDisable(GL_BLEND);
                    glDisable(GL_ALPHA_TEST);
                    glDepthMask(GL_TRUE);
                }
                Layer::Cutout => {
                    glDisable(GL_BLEND);
                    glEnable(GL_ALPHA_TEST);
                    glAlphaFunc(GL_GREATER, ALPHA_CUTOFF);
                    glDepthMask(GL_TRUE);
                }
                Layer::Translucent => {
                    glEnable(GL_BLEND);
                    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
                    glDisable(GL_ALPHA_TEST);
                    glDepthMask(GL_FALSE);
                }
            }
            glLoadMatrixf(mvp.0.as_ptr());
            glBindBuffer(GL_ARRAY_BUFFER, mesh.buffers[0]);
            glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, mesh.buffers[1]);
            for range in ranges {
                let stride = VERTEX_BYTES as GLsizei;
                glVertexPointer(3, GL_FLOAT, stride, offset_pointer(range.vertex_offset));
                glTexCoordPointer(
                    2,
                    GL_FLOAT,
                    stride,
                    offset_pointer(range.vertex_offset + 12),
                );
                glColorPointer(
                    4,
                    GL_UNSIGNED_BYTE,
                    stride,
                    offset_pointer(range.vertex_offset + 20),
                );
                glDrawElements(
                    GL_TRIANGLES,
                    range.index_count,
                    GL_UNSIGNED_SHORT,
                    offset_pointer(range.index_offset),
                );
            }
        }
    }

    fn end_frame(&mut self) -> Result<(), GfxError> {
        if !self.has_atlas {
            return Err(GfxError::NoAtlas);
        }
        // SAFETY: presents the frame; no common dialog is in use.
        unsafe { vglSwapBuffers(GL_FALSE) };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use psv_render::mesh::{Batch, Vertex};

    use super::*;

    #[test]
    fn meshes_are_packed_into_24_byte_vertices_with_recorded_ranges() {
        let vertex = |x: f32| Vertex {
            pos: [x, 2.0, 3.0],
            uv: [0.25, 0.5],
            color: [10, 20, 30, 255],
        };
        let batch = Batch {
            vertices: vec![vertex(1.0), vertex(2.0), vertex(3.0), vertex(4.0)],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        let mut mesh = Mesh::default();
        mesh.layers[Layer::Solid.index()] = vec![batch.clone(), batch.clone()];
        mesh.layers[Layer::Translucent.index()] = vec![batch];

        let (vertices, indices, layers) = pack_mesh(&mesh);
        assert_eq!(vertices.len(), 12 * VERTEX_BYTES);
        assert_eq!(indices.len(), 18 * 2);
        let solid = &layers[Layer::Solid.index()];
        assert_eq!(solid.len(), 2);
        assert_eq!(
            (solid[1].vertex_offset, solid[1].index_offset),
            (4 * VERTEX_BYTES, 12)
        );
        assert_eq!(
            layers[Layer::Translucent.index()][0].vertex_offset,
            8 * VERTEX_BYTES
        );
        assert!(layers[Layer::Cutout.index()].is_empty());

        assert_eq!(f32::from_le_bytes(vertices[0..4].try_into().unwrap()), 1.0);
        assert_eq!(
            f32::from_le_bytes(vertices[12..16].try_into().unwrap()),
            0.25
        );
        assert_eq!(&vertices[20..24], &[10, 20, 30, 255]);
        assert_eq!(u16::from_le_bytes(indices[4..6].try_into().unwrap()), 2);
    }

    #[test]
    fn the_controller_record_is_large_enough_for_the_console() {
        assert!(std::mem::size_of::<CtrlData>() >= 32);
    }
}
