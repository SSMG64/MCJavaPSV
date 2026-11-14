//! Everything the Vita renderer needs that does not touch the GPU.
//!
//! It turns baked textures into an atlas, block model JSON into quads, blocks into chunk
//! meshes, and a position and two angles into the matrices a draw call needs. None of it
//! knows about GXM, so all of it runs and is tested on a PC. A small software rasterizer in
//! [`raster`] lets the tests check what a mesh looks like without a GPU.

pub mod atlas;
pub mod blockstate;
pub mod camera;
pub mod geometry;
pub mod gpu;
pub mod math;
pub mod mesh;
pub mod model;
pub mod quad;
pub mod raster;
pub mod registry;

pub use geometry::{Direction, Rotation};
