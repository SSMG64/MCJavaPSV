//! Chunk meshing: blocks in, vertex and index buffers out, one set per render layer.

use crate::geometry::Direction;
use crate::quad::{BakedQuad, Layer, occlusion};

/// Meshes use 16 bit indices, which is what the Vita's GPU takes, so a batch stops growing
/// just short of 65,536 vertices.
pub const MAX_BATCH_VERTICES: usize = 65_532;

#[derive(Debug, Clone, PartialEq)]
pub struct Appearance {
    pub quads: Vec<BakedQuad>,
    pub occludes: [bool; 6],
}

impl Appearance {
    pub fn new(quads: Vec<BakedQuad>) -> Self {
        let occludes = occlusion(&quads);
        Self { quads, occludes }
    }
}

/// How a block state looks. `None` means nothing is drawn, as for air.
pub trait AppearanceProvider {
    fn appearance(&self, state: u32) -> Option<&Appearance>;
}

pub trait BlockView {
    fn state(&self, x: i32, y: i32, z: i32) -> u32;
}

/// Biome and block colors for faces that ask for a tint, as red, green and blue from 0 to 1.
pub trait Tinter {
    fn tint(&self, index: i32, x: i32, y: i32, z: i32) -> [f32; 3];
}

pub struct NoTint;

impl Tinter for NoTint {
    fn tint(&self, _index: i32, _x: i32, _y: i32, _z: i32) -> [f32; 3] {
        [1.0; 3]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Batch {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Indexed by [`Layer::index`].
    pub layers: [Vec<Batch>; 3],
}

impl Mesh {
    pub fn quad_count(&self) -> usize {
        self.layers
            .iter()
            .flatten()
            .map(|b| b.indices.len() / 6)
            .sum()
    }

    pub fn vertex_count(&self) -> usize {
        self.layers.iter().flatten().map(|b| b.vertices.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.quad_count() == 0
    }

    pub fn layer(&self, layer: Layer) -> &[Batch] {
        &self.layers[layer.index()]
    }

    fn push(&mut self, layer: Layer, corners: [Vertex; 4]) {
        let batches = &mut self.layers[layer.index()];
        if batches
            .last()
            .is_none_or(|b| b.vertices.len() + 4 > MAX_BATCH_VERTICES)
        {
            batches.push(Batch::default());
        }
        let Some(batch) = batches.last_mut() else {
            return;
        };
        let base = batch.vertices.len() as u16;
        batch.vertices.extend_from_slice(&corners);
        batch
            .indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Whether the neighbour in `direction` hides this quad.
fn hidden(
    view: &dyn BlockView,
    provider: &dyn AppearanceProvider,
    state: u32,
    at: [i32; 3],
    quad: &BakedQuad,
) -> bool {
    let Some(direction) = quad.cull else {
        return false;
    };
    let n = direction.normal();
    let neighbour = view.state(at[0] + n[0], at[1] + n[1], at[2] + n[2]);
    if neighbour == state && quad.layer == Layer::Translucent {
        return true;
    }
    provider
        .appearance(neighbour)
        .is_some_and(|a| a.occludes[direction.opposite().index()])
}

/// Meshes the blocks of the box starting at `min` and `size` blocks across. Vertex positions
/// are relative to `min`, so a caller moves the mesh into place with one translation.
/// Faces against blocks outside the box are judged by asking `view`, so neighbouring
/// sections join without seams.
pub fn mesh_region(
    view: &dyn BlockView,
    provider: &dyn AppearanceProvider,
    tinter: &dyn Tinter,
    min: [i32; 3],
    size: [i32; 3],
) -> Mesh {
    let mut mesh = Mesh::default();
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let at = [min[0] + x, min[1] + y, min[2] + z];
                let state = view.state(at[0], at[1], at[2]);
                let Some(appearance) = provider.appearance(state) else {
                    continue;
                };
                for quad in &appearance.quads {
                    if hidden(view, provider, state, at, quad) {
                        continue;
                    }
                    let light = if quad.shade { quad.face.shade() } else { 1.0 };
                    let tint = if quad.tint >= 0 {
                        tinter.tint(quad.tint, at[0], at[1], at[2])
                    } else {
                        [1.0; 3]
                    };
                    let color = [
                        byte(tint[0] * light),
                        byte(tint[1] * light),
                        byte(tint[2] * light),
                        255,
                    ];
                    let corners = std::array::from_fn(|i| Vertex {
                        pos: [
                            (x as f32) + quad.corners[i][0],
                            (y as f32) + quad.corners[i][1],
                            (z as f32) + quad.corners[i][2],
                        ],
                        uv: quad.uvs[i],
                        color,
                    });
                    mesh.push(quad.layer, corners);
                }
            }
        }
    }
    mesh
}

/// The neighbouring block coordinate in a direction.
pub fn offset(at: [i32; 3], direction: Direction) -> [i32; 3] {
    let n = direction.normal();
    [at[0] + n[0], at[1] + n[1], at[2] + n[2]]
}
