//! Baking: a resolved model, a rotation and an atlas in, positioned and textured quads out.

use crate::atlas::{Atlas, UvRect};
use crate::geometry::{Direction, Rotation};
use crate::math::{cross, normalize, sub};
use crate::model::{Element, ElementRotation, Face, ResolvedModel};
use psv_pack::texture::{FLAG_HAS_TRANSLUCENT, FLAG_HAS_TRANSPARENT};

/// Which pass a quad is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Solid,
    /// Fully transparent or fully opaque pixels: leaves, glass panes, flowers.
    Cutout,
    /// Partly transparent pixels: water, stained glass, ice.
    Translucent,
}

impl Layer {
    pub const ALL: [Layer; 3] = [Layer::Solid, Layer::Cutout, Layer::Translucent];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_flags(flags: u8) -> Self {
        if flags & FLAG_HAS_TRANSLUCENT != 0 {
            Self::Translucent
        } else if flags & FLAG_HAS_TRANSPARENT != 0 {
            Self::Cutout
        } else {
            Self::Solid
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BakedQuad {
    /// Corners in block space (0 to 1), counter-clockwise seen from outside.
    pub corners: [[f32; 3]; 4],
    /// Atlas texture coordinates of each corner.
    pub uvs: [[f32; 2]; 4],
    /// The way the quad faces, used for shading.
    pub face: Direction,
    /// The neighbour whose full face hides this quad.
    pub cull: Option<Direction>,
    pub shade: bool,
    pub tint: i32,
    pub layer: Layer,
}

pub trait Sprites {
    fn uv(&self, texture: &str) -> Option<UvRect>;
    fn flags(&self, texture: &str) -> u8;
}

impl Sprites for Atlas {
    fn uv(&self, texture: &str) -> Option<UvRect> {
        Atlas::uv(self, texture)
    }

    fn flags(&self, texture: &str) -> u8 {
        self.sprite(texture).map_or(0, |s| s.flags)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BakeReport {
    pub unresolved_variables: Vec<String>,
    pub missing_sprites: Vec<String>,
}

/// Corners of each face, 0 for the box's minimum and 1 for its maximum, per axis. The order
/// is the model format's: top-left, bottom-left, bottom-right, top-right seen from outside.
const CORNERS: [[[u8; 3]; 4]; 6] = [
    [[0, 0, 1], [0, 0, 0], [1, 0, 0], [1, 0, 1]],
    [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]],
    [[1, 1, 0], [1, 0, 0], [0, 0, 0], [0, 1, 0]],
    [[0, 1, 1], [0, 0, 1], [1, 0, 1], [1, 1, 1]],
    [[0, 1, 0], [0, 0, 0], [0, 0, 1], [0, 1, 1]],
    [[1, 1, 1], [1, 0, 1], [1, 0, 0], [1, 1, 0]],
];

fn default_uv(direction: Direction, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    match direction {
        Direction::Down => [from[0], 16.0 - to[2], to[0], 16.0 - from[2]],
        Direction::Up => [from[0], from[2], to[0], to[2]],
        Direction::North => [16.0 - to[0], 16.0 - to[1], 16.0 - from[0], 16.0 - from[1]],
        Direction::South => [from[0], 16.0 - to[1], to[0], 16.0 - from[1]],
        Direction::West => [from[2], 16.0 - to[1], to[2], 16.0 - from[1]],
        Direction::East => [16.0 - to[2], 16.0 - to[1], 16.0 - from[2], 16.0 - from[1]],
    }
}

type Matrix3 = [[f32; 3]; 3];

fn axis_rotation(axis: usize, degrees: f32) -> Matrix3 {
    let (s, c) = degrees.to_radians().sin_cos();
    match axis {
        0 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        1 => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    }
}

fn multiply(a: &Matrix3, b: &Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, slot) in row.iter_mut().enumerate() {
            *slot = (0..3).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

fn apply(m: &Matrix3, v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// The element rotation as one matrix. With `rescale`, the geometry is first stretched by the
/// reciprocal of the largest component of each rotated axis, which keeps a tilted plane
/// reaching the block's edges.
fn element_matrix(rotation: &ElementRotation) -> (Matrix3, [f32; 3]) {
    let (matrix, origin, rescale) = match rotation {
        ElementRotation::Axis {
            origin,
            axis,
            degrees,
            rescale,
        } => (axis_rotation(*axis, *degrees), *origin, *rescale),
        ElementRotation::Euler {
            origin,
            degrees,
            rescale,
        } => {
            let zy = multiply(&axis_rotation(2, degrees[2]), &axis_rotation(1, degrees[1]));
            (
                multiply(&zy, &axis_rotation(0, degrees[0])),
                *origin,
                *rescale,
            )
        }
    };
    if !rescale {
        return (matrix, origin);
    }
    let mut scale = [1.0; 3];
    for (axis, slot) in scale.iter_mut().enumerate() {
        let mut unit = [0.0; 3];
        unit[axis] = 1.0;
        let r = apply(&matrix, unit);
        let largest = r[0].abs().max(r[1].abs()).max(r[2].abs());
        if largest > 0.0 {
            *slot = 1.0 / largest;
        }
    }
    let scaled = [
        [
            matrix[0][0] * scale[0],
            matrix[0][1] * scale[1],
            matrix[0][2] * scale[2],
        ],
        [
            matrix[1][0] * scale[0],
            matrix[1][1] * scale[1],
            matrix[1][2] * scale[2],
        ],
        [
            matrix[2][0] * scale[0],
            matrix[2][1] * scale[1],
            matrix[2][2] * scale[2],
        ],
    ];
    (scaled, origin)
}

/// The face direction a quad most nearly points in, from its corners.
fn facing(corners: &[[f32; 3]; 4], fallback: Direction) -> Direction {
    let normal = normalize(cross(
        sub(corners[1], corners[0]),
        sub(corners[2], corners[1]),
    ));
    if normal == [0.0; 3] {
        return fallback;
    }
    Direction::ALL
        .into_iter()
        .max_by(|a, b| {
            let score = |d: &Direction| {
                let n = d.normal();
                normal[0] * n[0] as f32 + normal[1] * n[1] as f32 + normal[2] * n[2] as f32
            };
            score(a).total_cmp(&score(b))
        })
        .unwrap_or(fallback)
}

type Int3 = [[i32; 3]; 3];

fn transpose(m: &Int3) -> Int3 {
    std::array::from_fn(|r| std::array::from_fn(|c| m[c][r]))
}

fn multiply_int(a: &Int3, b: &Int3) -> Int3 {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

/// Turns a face's own frame (where it looks like a south face) into the block's frame.
fn face_to_block(direction: Direction) -> Int3 {
    match direction {
        Direction::South => [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        Direction::East => [[0, 0, 1], [0, 1, 0], [-1, 0, 0]],
        Direction::West => [[0, 0, -1], [0, 1, 0], [1, 0, 0]],
        Direction::North => [[-1, 0, 0], [0, 1, 0], [0, 0, -1]],
        Direction::Up => [[1, 0, 0], [0, 0, 1], [0, -1, 0]],
        Direction::Down => [[1, 0, 0], [0, 0, -1], [0, 1, 0]],
    }
}

fn rotation_matrix(rotation: Rotation) -> Int3 {
    let columns = [[1, 0, 0], [0, 1, 0], [0, 0, 1]].map(|axis| rotation.apply_i32(axis));
    std::array::from_fn(|r| std::array::from_fn(|c| columns[c][r]))
}

/// With uvlock, a face's texture stays fixed to the world while the block turns. This is the
/// rotation, in the face's own frame, that undoes what the block's turn did to the face.
fn uv_lock_transform(direction: Direction, rotation: Rotation) -> Int3 {
    let action = multiply_int(&rotation_matrix(rotation), &face_to_block(direction));
    let normal = [action[0][2], action[1][2], action[2][2]];
    let new_side = Direction::from_normal(normal).unwrap_or(direction);
    let face_rotation = multiply_int(&transpose(&face_to_block(new_side)), &action);
    transpose(&face_rotation)
}

fn lock_uv(transform: &Int3, u: f32, v: f32) -> (f32, f32) {
    let (x, y) = (u - 0.5, v - 0.5);
    let tx = transform[0][0] as f32 * x + transform[0][1] as f32 * y;
    let ty = transform[1][0] as f32 * x + transform[1][1] as f32 * y;
    (tx + 0.5, ty + 0.5)
}

fn bake_face(
    element: &Element,
    direction: Direction,
    face: &Face,
    model: &ResolvedModel,
    options: (Rotation, bool),
    sprites: &dyn Sprites,
    report: &mut BakeReport,
) -> Option<BakedQuad> {
    let (rotation, uvlock) = options;
    let Some(resolved) = model.resolve_texture(&face.texture) else {
        report.unresolved_variables.push(face.texture.clone());
        return None;
    };
    let texture = resolved.location;
    let Some(rect) = sprites.uv(&texture) else {
        report.missing_sprites.push(texture);
        return None;
    };

    let mut corners = [[0.0_f32; 3]; 4];
    for (corner, bits) in corners.iter_mut().zip(CORNERS[direction.index()]) {
        for axis in 0..3 {
            corner[axis] = if bits[axis] == 0 {
                element.from[axis]
            } else {
                element.to[axis]
            };
        }
    }
    if let Some(element_rotation) = &element.rotation {
        let (matrix, origin) = element_matrix(element_rotation);
        for corner in &mut corners {
            let local = [
                corner[0] - origin[0],
                corner[1] - origin[1],
                corner[2] - origin[2],
            ];
            let r = apply(&matrix, local);
            *corner = [r[0] + origin[0], r[1] + origin[1], r[2] + origin[2]];
        }
    }
    for corner in &mut corners {
        *corner = [corner[0] / 16.0, corner[1] / 16.0, corner[2] / 16.0];
    }
    let nominal = facing(&corners, direction);

    let [u0, v0, u1, v1] = face
        .uv
        .unwrap_or_else(|| default_uv(direction, element.from, element.to));
    let corner_uv = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
    let lock = (uvlock && !rotation.is_identity()).then(|| uv_lock_transform(direction, rotation));
    let mut uvs = [[0.0; 2]; 4];
    for (i, slot) in uvs.iter_mut().enumerate() {
        let [u, v] = corner_uv[(i + usize::from(face.rotation)) % 4];
        let (mut u, mut v) = (u / 16.0, v / 16.0);
        if let Some(transform) = &lock {
            (u, v) = lock_uv(transform, u, v);
        }
        *slot = [
            rect.u0 + u * (rect.u1 - rect.u0),
            rect.v0 + v * (rect.v1 - rect.v0),
        ];
    }

    if !rotation.is_identity() {
        for corner in &mut corners {
            *corner = rotation.apply_point(*corner);
        }
    }
    Some(BakedQuad {
        corners,
        uvs,
        face: rotation.apply_direction(nominal),
        cull: face.cull.map(|d| rotation.apply_direction(d)),
        shade: element.shade,
        tint: face.tint,
        layer: if resolved.force_translucent {
            Layer::Translucent
        } else {
            Layer::from_flags(sprites.flags(&texture))
        },
    })
}

/// Bakes every face of every element. Faces whose texture cannot be found are left out and
/// listed in the report.
pub fn bake(
    model: &ResolvedModel,
    rotation: Rotation,
    sprites: &dyn Sprites,
) -> (Vec<BakedQuad>, BakeReport) {
    bake_locked(model, rotation, false, sprites)
}

/// Like [`bake`], and with `uvlock` the textures stay aligned to the world instead of turning
/// with the block.
pub fn bake_locked(
    model: &ResolvedModel,
    rotation: Rotation,
    uvlock: bool,
    sprites: &dyn Sprites,
) -> (Vec<BakedQuad>, BakeReport) {
    let mut quads = Vec::new();
    let mut report = BakeReport::default();
    for element in &model.elements {
        for direction in Direction::ALL {
            if let Some(face) = &element.faces[direction.index()]
                && let Some(quad) = bake_face(
                    element,
                    direction,
                    face,
                    model,
                    (rotation, uvlock),
                    sprites,
                    &mut report,
                )
            {
                quads.push(quad);
            }
        }
    }
    (quads, report)
}

/// For each direction, whether these quads completely cover that side of the block with
/// opaque texture. A neighbour uses this to decide whether its own face is hidden.
pub fn occlusion(quads: &[BakedQuad]) -> [bool; 6] {
    let mut covered = [false; 6];
    for quad in quads.iter().filter(|q| q.layer == Layer::Solid) {
        for direction in Direction::ALL {
            let n = direction.normal();
            let axis = if n[0] != 0 {
                0
            } else if n[1] != 0 {
                1
            } else {
                2
            };
            let plane = if n[axis] > 0 { 1.0 } else { 0.0 };
            let on_plane = quad.corners.iter().all(|c| (c[axis] - plane).abs() < 1e-4);
            if !on_plane {
                continue;
            }
            let full = (0..3).filter(|&a| a != axis).all(|a| {
                let low = quad.corners.iter().map(|c| c[a]).fold(f32::MAX, f32::min);
                let high = quad.corners.iter().map(|c| c[a]).fold(f32::MIN, f32::max);
                low < 1e-4 && high > 1.0 - 1e-4
            });
            covered[direction.index()] |= full;
        }
    }
    covered
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::model::{ModelSource, resolve};

    pub struct Fake(pub HashMap<String, String>);

    impl ModelSource for Fake {
        fn load(&mut self, location: &str) -> Option<String> {
            self.0.get(location).cloned()
        }
    }

    pub struct Grid(pub HashMap<String, (UvRect, u8)>);

    impl Sprites for Grid {
        fn uv(&self, texture: &str) -> Option<UvRect> {
            self.0.get(texture).map(|(r, _)| *r)
        }

        fn flags(&self, texture: &str) -> u8 {
            self.0.get(texture).map_or(0, |(_, f)| *f)
        }
    }

    pub fn whole(name: &str, flags: u8) -> Grid {
        let rect = UvRect {
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
        };
        Grid([(name.to_owned(), (rect, flags))].into())
    }

    pub fn cube(json: &str) -> ResolvedModel {
        let mut source = Fake([("minecraft:m".to_owned(), json.to_owned())].into());
        resolve("m", &mut source).unwrap()
    }

    const FULL_CUBE: &str = r##"{
        "textures": {"all": "t"},
        "elements": [{"from":[0,0,0],"to":[16,16,16],"faces":{
            "down":{"texture":"#all","cullface":"down"},
            "up":{"texture":"#all","cullface":"up"},
            "north":{"texture":"#all","cullface":"north"},
            "south":{"texture":"#all","cullface":"south"},
            "west":{"texture":"#all","cullface":"west"},
            "east":{"texture":"#all","cullface":"east"}}}]
    }"##;

    #[test]
    fn a_full_cube_bakes_six_unit_quads_facing_outward() {
        let (quads, report) = bake(&cube(FULL_CUBE), Rotation::NONE, &whole("minecraft:t", 0));
        assert_eq!(report, BakeReport::default());
        assert_eq!(quads.len(), 6);
        for quad in &quads {
            let n = quad.face.normal();
            let normal = normalize(cross(
                sub(quad.corners[1], quad.corners[0]),
                sub(quad.corners[2], quad.corners[1]),
            ));
            assert_eq!(
                [normal[0] as i32, normal[1] as i32, normal[2] as i32],
                n,
                "{:?}",
                quad.face
            );
            assert_eq!(quad.cull, Some(quad.face));
            for c in &quad.corners {
                assert!(c.iter().all(|v| (0.0..=1.0).contains(v)));
            }
        }
        assert_eq!(occlusion(&quads), [true; 6]);
    }

    #[test]
    fn default_uvs_put_the_texture_upright_on_each_side() {
        let (quads, _) = bake(&cube(FULL_CUBE), Rotation::NONE, &whole("minecraft:t", 0));
        let south = quads.iter().find(|q| q.face == Direction::South).unwrap();
        assert_eq!(south.uvs, [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
        let up = quads.iter().find(|q| q.face == Direction::Up).unwrap();
        assert_eq!(up.corners[0], [0.0, 1.0, 0.0]);
        assert_eq!(up.uvs[0], [0.0, 0.0]);
        assert_eq!(up.uvs[2], [1.0, 1.0]);
    }

    #[test]
    fn texture_rotation_shifts_the_corners() {
        let json = r##"{"textures":{"a":"t"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
            "south":{"texture":"#a","rotation":90}}}]}"##;
        let (quads, _) = bake(&cube(json), Rotation::NONE, &whole("minecraft:t", 0));
        assert_eq!(
            quads[0].uvs,
            [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]
        );
    }

    #[test]
    fn explicit_uvs_map_into_the_sprite_rectangle() {
        let json = r##"{"textures":{"a":"t"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
            "south":{"texture":"#a","uv":[0,0,8,8]}}}]}"##;
        let sprites = Grid(
            [(
                "minecraft:t".to_owned(),
                (
                    UvRect {
                        u0: 0.5,
                        v0: 0.25,
                        u1: 0.75,
                        v1: 0.5,
                    },
                    0,
                ),
            )]
            .into(),
        );
        let (quads, _) = bake(&cube(json), Rotation::NONE, &sprites);
        assert_eq!(quads[0].uvs[0], [0.5, 0.25]);
        assert_eq!(quads[0].uvs[2], [0.625, 0.375]);
    }

    #[test]
    fn blockstate_rotation_turns_quads_and_their_cull_faces() {
        let (quads, _) = bake(
            &cube(FULL_CUBE),
            Rotation::from_degrees(0, 90).unwrap(),
            &whole("minecraft:t", 0),
        );
        let east = quads
            .iter()
            .find(|q| q.cull == Some(Direction::East))
            .unwrap();
        assert_eq!(east.face, Direction::East);
        assert!(east.corners.iter().all(|c| (c[0] - 1.0).abs() < 1e-5));
    }

    #[test]
    fn partial_elements_do_not_occlude_their_neighbours() {
        let slab = r##"{"textures":{"a":"t"},"elements":[{"from":[0,0,0],"to":[16,8,16],"faces":{
            "down":{"texture":"#a","cullface":"down"},"up":{"texture":"#a"},
            "south":{"texture":"#a","cullface":"south"}}}]}"##;
        let (quads, _) = bake(&cube(slab), Rotation::NONE, &whole("minecraft:t", 0));
        let covered = occlusion(&quads);
        assert!(covered[Direction::Down.index()]);
        assert!(!covered[Direction::Up.index()]);
        assert!(!covered[Direction::South.index()]);
    }

    #[test]
    fn translucent_and_cutout_textures_do_not_occlude() {
        let glass = whole("minecraft:t", FLAG_HAS_TRANSPARENT);
        let (quads, _) = bake(&cube(FULL_CUBE), Rotation::NONE, &glass);
        assert!(quads.iter().all(|q| q.layer == Layer::Cutout));
        assert_eq!(occlusion(&quads), [false; 6]);
        let water = whole("minecraft:t", FLAG_HAS_TRANSLUCENT | FLAG_HAS_TRANSPARENT);
        assert_eq!(
            bake(&cube(FULL_CUBE), Rotation::NONE, &water).0[0].layer,
            Layer::Translucent
        );
    }

    #[test]
    fn a_tilted_plane_with_rescale_still_spans_the_block() {
        let cross_model = r##"{"textures":{"a":"t"},"elements":[{"from":[0.8,0,8],"to":[15.2,16,8],
            "rotation":{"origin":[8,8,8],"axis":"y","angle":45,"rescale":true},
            "faces":{"north":{"texture":"#a"},"south":{"texture":"#a"}}}]}"##;
        let (quads, _) = bake(&cube(cross_model), Rotation::NONE, &whole("minecraft:t", 0));
        assert_eq!(quads.len(), 2);
        let xs: Vec<f32> = quads[0].corners.iter().map(|c| c[0]).collect();
        let spread = xs.iter().cloned().fold(f32::MIN, f32::max)
            - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!((spread - 0.9).abs() < 1e-3, "rotated plane spans {spread}");
        assert!(matches!(
            quads[0].face,
            Direction::North | Direction::South | Direction::East | Direction::West
        ));
    }

    #[test]
    fn unresolvable_textures_are_reported_not_drawn() {
        let json = r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
            "up":{"texture":"#undefined"},"down":{"texture":"nothere"}}}]}"##;
        let (quads, report) = bake(&cube(json), Rotation::NONE, &whole("minecraft:t", 0));
        assert!(quads.is_empty());
        assert_eq!(report.unresolved_variables, ["#undefined"]);
        assert_eq!(report.missing_sprites, ["minecraft:nothere"]);
    }

    fn positions_to_uvs(quads: &[BakedQuad]) -> HashMap<(Direction, [i32; 3]), [i32; 2]> {
        let mut map = HashMap::new();
        for quad in quads {
            for (corner, uv) in quad.corners.iter().zip(quad.uvs) {
                let key = (quad.face, corner.map(|c| (c * 1000.0).round() as i32));
                map.insert(key, uv.map(|c| (c * 1000.0).round() as i32));
            }
        }
        map
    }

    #[test]
    fn uvlock_keeps_a_cubes_texture_coordinates_where_they_were_for_every_rotation() {
        let sprites = whole("minecraft:t", 0);
        let model = cube(FULL_CUBE);
        let baseline = positions_to_uvs(&bake(&model, Rotation::NONE, &sprites).0);
        let mut unlocked_differs = false;
        for x in 0..4 {
            for y in 0..4 {
                let rotation = Rotation {
                    x_turns: x,
                    y_turns: y,
                };
                let locked = positions_to_uvs(&bake_locked(&model, rotation, true, &sprites).0);
                assert_eq!(locked, baseline, "x turns {x}, y turns {y}");
                let unlocked = positions_to_uvs(&bake(&model, rotation, &sprites).0);
                unlocked_differs |= unlocked != baseline;
            }
        }
        assert!(unlocked_differs, "the check would pass without uvlock too");
    }

    #[test]
    fn uvlock_does_nothing_without_a_rotation() {
        let sprites = whole("minecraft:t", 0);
        let model = cube(FULL_CUBE);
        assert_eq!(
            bake_locked(&model, Rotation::NONE, true, &sprites).0,
            bake(&model, Rotation::NONE, &sprites).0
        );
    }

    #[test]
    fn a_forced_translucent_slot_makes_an_opaque_texture_translucent() {
        let json = r##"{"textures":{"a":{"sprite":"t","force_translucent":true}},"elements":[
            {"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#a","cullface":"up"}}}]}"##;
        let (quads, _) = bake(&cube(json), Rotation::NONE, &whole("minecraft:t", 0));
        assert_eq!(quads[0].layer, Layer::Translucent);
        assert_eq!(occlusion(&quads), [false; 6]);
    }
}
