/// The six faces of a block, in the order the model format lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    pub const ALL: [Direction; 6] = [
        Direction::Down,
        Direction::Up,
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "down" => Self::Down,
            "up" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    /// Unit vector pointing out of the face. North is -Z, east is +X.
    pub fn normal(self) -> [i32; 3] {
        match self {
            Self::Down => [0, -1, 0],
            Self::Up => [0, 1, 0],
            Self::North => [0, 0, -1],
            Self::South => [0, 0, 1],
            Self::West => [-1, 0, 0],
            Self::East => [1, 0, 0],
        }
    }

    pub fn from_normal(normal: [i32; 3]) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.normal() == normal)
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// Brightness of a face in the overworld, which is what makes tops lighter than sides.
    pub fn shade(self) -> f32 {
        match self {
            Self::Down => 0.5,
            Self::Up => 1.0,
            Self::North | Self::South => 0.8,
            Self::West | Self::East => 0.6,
        }
    }
}

/// A blockstate rotation: quarter turns about X, then about Y. Each turn goes the way the
/// game's `"x"` and `"y"` angles do: +90 on Y takes north to east.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rotation {
    pub x_turns: u8,
    pub y_turns: u8,
}

impl Rotation {
    pub const NONE: Rotation = Rotation {
        x_turns: 0,
        y_turns: 0,
    };

    /// `x` and `y` are the angles from a blockstate file. Anything but a multiple of 90 fails.
    pub fn from_degrees(x: i32, y: i32) -> Option<Self> {
        let turns = |degrees: i32| (degrees % 90 == 0).then(|| (degrees / 90).rem_euclid(4) as u8);
        Some(Self {
            x_turns: turns(x)?,
            y_turns: turns(y)?,
        })
    }

    pub fn is_identity(self) -> bool {
        self == Self::NONE
    }

    pub fn apply_f32(self, mut v: [f32; 3]) -> [f32; 3] {
        for _ in 0..self.x_turns {
            v = [v[0], v[2], -v[1]];
        }
        for _ in 0..self.y_turns {
            v = [-v[2], v[1], v[0]];
        }
        v
    }

    pub fn apply_i32(self, mut v: [i32; 3]) -> [i32; 3] {
        for _ in 0..self.x_turns {
            v = [v[0], v[2], -v[1]];
        }
        for _ in 0..self.y_turns {
            v = [-v[2], v[1], v[0]];
        }
        v
    }

    pub fn apply_direction(self, direction: Direction) -> Direction {
        Direction::from_normal(self.apply_i32(direction.normal())).unwrap_or(direction)
    }

    /// Rotates a point of the unit block around its centre.
    pub fn apply_point(self, p: [f32; 3]) -> [f32; 3] {
        let r = self.apply_f32([p[0] - 0.5, p[1] - 0.5, p[2] - 0.5]);
        [r[0] + 0.5, r[1] + 0.5, r[2] + 0.5]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normals_round_trip_and_opposites_cancel() {
        for d in Direction::ALL {
            assert_eq!(Direction::from_normal(d.normal()), Some(d));
            let (a, b) = (d.normal(), d.opposite().normal());
            assert_eq!([a[0] + b[0], a[1] + b[1], a[2] + b[2]], [0, 0, 0]);
            assert_eq!(Direction::parse(&format!("{d:?}").to_lowercase()), Some(d));
        }
        assert_eq!(Direction::parse("sideways"), None);
    }

    #[test]
    fn shading_matches_the_overworld_values() {
        let shades: Vec<f32> = Direction::ALL.iter().map(|d| d.shade()).collect();
        assert_eq!(shades, [0.5, 1.0, 0.8, 0.8, 0.6, 0.6]);
    }

    #[test]
    fn y_rotation_turns_clockwise_seen_from_above() {
        let r = Rotation::from_degrees(0, 90).unwrap();
        assert_eq!(r.apply_direction(Direction::North), Direction::East);
        assert_eq!(r.apply_direction(Direction::East), Direction::South);
        assert_eq!(r.apply_direction(Direction::South), Direction::West);
        assert_eq!(r.apply_direction(Direction::West), Direction::North);
        assert_eq!(r.apply_direction(Direction::Up), Direction::Up);
    }

    #[test]
    fn x_rotation_tips_the_top_toward_the_north() {
        let r = Rotation::from_degrees(90, 0).unwrap();
        assert_eq!(r.apply_direction(Direction::Up), Direction::North);
        assert_eq!(r.apply_direction(Direction::North), Direction::Down);
        assert_eq!(r.apply_direction(Direction::East), Direction::East);
    }

    #[test]
    fn x_is_applied_before_y() {
        let r = Rotation::from_degrees(90, 90).unwrap();
        assert_eq!(r.apply_direction(Direction::Up), Direction::East);
    }

    #[test]
    fn four_turns_are_the_identity_and_bad_angles_fail() {
        let r = Rotation::from_degrees(360, -360).unwrap();
        assert!(r.is_identity());
        assert_eq!(
            Rotation::from_degrees(0, 270),
            Rotation::from_degrees(0, -90)
        );
        assert_eq!(Rotation::from_degrees(45, 0), None);
    }

    #[test]
    fn points_rotate_about_the_block_centre() {
        let r = Rotation::from_degrees(0, 90).unwrap();
        assert_eq!(r.apply_point([0.5, 0.0, 0.0]), [1.0, 0.0, 0.5]);
        assert_eq!(r.apply_point([0.5, 0.5, 0.5]), [0.5, 0.5, 0.5]);
    }
}
