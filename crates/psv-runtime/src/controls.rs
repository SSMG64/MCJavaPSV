//! Turns the controller into camera movement. It knows nothing about the console, so it is
//! tested on a PC.

use psv_render::camera::{Camera, FlyInput};

pub const BUTTON_START: u32 = 0x0000_0008;
pub const BUTTON_L: u32 = 0x0000_0100;
pub const BUTTON_R: u32 = 0x0000_0200;
pub const BUTTON_CROSS: u32 = 0x0000_4000;

const DEADZONE: f32 = 0.2;
const TURN_DEGREES_PER_SECOND: f32 = 120.0;

/// A controller reading: buttons, and sticks from 0 to 255 with 128 at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pad {
    pub buttons: u32,
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
}

impl Pad {
    pub const REST: Pad = Pad {
        buttons: 0,
        lx: 128,
        ly: 128,
        rx: 128,
        ry: 128,
    };

    fn pressed(&self, button: u32) -> bool {
        self.buttons & button != 0
    }
}

fn axis(raw: u8) -> f32 {
    let value = (f32::from(raw) - 128.0) / 128.0;
    if value.abs() < DEADZONE { 0.0 } else { value }
}

pub fn wants_exit(pad: &Pad) -> bool {
    pad.pressed(BUTTON_START)
}

/// Left stick walks, right stick looks, the shoulder buttons fly down and up, and cross sprints.
pub fn update(camera: &mut Camera, pad: &Pad, seconds: f32) {
    camera.look(
        axis(pad.rx) * TURN_DEGREES_PER_SECOND * seconds,
        axis(pad.ry) * TURN_DEGREES_PER_SECOND * 0.75 * seconds,
    );
    let rise =
        f32::from(u8::from(pad.pressed(BUTTON_R))) - f32::from(u8::from(pad.pressed(BUTTON_L)));
    camera.fly(
        FlyInput {
            forward: -axis(pad.ly),
            strafe: axis(pad.lx),
            rise,
            sprint: pad.pressed(BUTTON_CROSS),
        },
        seconds,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        Camera::new([0.0; 3], 1.0)
    }

    #[test]
    fn a_resting_pad_changes_nothing() {
        let mut c = camera();
        update(&mut c, &Pad::REST, 1.0);
        assert_eq!((c.position, c.yaw, c.pitch), ([0.0; 3], 0.0, 0.0));
    }

    #[test]
    fn small_stick_drift_is_ignored() {
        let mut c = camera();
        update(
            &mut c,
            &Pad {
                ly: 120,
                lx: 136,
                rx: 140,
                ry: 118,
                ..Pad::REST
            },
            1.0,
        );
        assert_eq!((c.position, c.yaw, c.pitch), ([0.0; 3], 0.0, 0.0));
    }

    #[test]
    fn pushing_the_left_stick_up_walks_forward_and_right_strafes_right() {
        let mut c = camera();
        c.yaw = 180.0;
        update(&mut c, &Pad { ly: 0, ..Pad::REST }, 1.0);
        assert!(
            (c.position[2] + 10.0).abs() < 1e-3,
            "north is -Z: {:?}",
            c.position
        );
        let mut c = camera();
        c.yaw = 180.0;
        update(
            &mut c,
            &Pad {
                lx: 255,
                ..Pad::REST
            },
            1.0,
        );
        assert!(
            c.position[0] > 9.0,
            "facing north, right is east: {:?}",
            c.position
        );
    }

    #[test]
    fn the_right_stick_turns_right_and_looks_up_or_down() {
        let mut c = camera();
        update(
            &mut c,
            &Pad {
                rx: 255,
                ..Pad::REST
            },
            0.5,
        );
        assert!(
            (c.yaw - 60.0).abs() < 1.0,
            "turning right increases yaw: {}",
            c.yaw
        );
        let mut c = camera();
        update(
            &mut c,
            &Pad {
                ry: 255,
                ..Pad::REST
            },
            0.5,
        );
        assert!(c.pitch > 0.0, "stick down looks down");
        let mut c = camera();
        update(&mut c, &Pad { ry: 0, ..Pad::REST }, 0.5);
        assert!(c.pitch < 0.0, "stick up looks up");
    }

    #[test]
    fn shoulder_buttons_fly_and_cross_sprints() {
        let mut c = camera();
        update(
            &mut c,
            &Pad {
                buttons: BUTTON_R,
                ..Pad::REST
            },
            1.0,
        );
        assert!(c.position[1] > 9.0);
        update(
            &mut c,
            &Pad {
                buttons: BUTTON_L,
                ..Pad::REST
            },
            1.0,
        );
        assert!(c.position[1].abs() < 1e-3);
        update(
            &mut c,
            &Pad {
                buttons: BUTTON_L | BUTTON_R,
                ..Pad::REST
            },
            1.0,
        );
        assert!(c.position[1].abs() < 1e-3, "both cancel");
        let mut walk = camera();
        let mut run = camera();
        update(&mut walk, &Pad { ly: 0, ..Pad::REST }, 1.0);
        update(
            &mut run,
            &Pad {
                ly: 0,
                buttons: BUTTON_CROSS,
                ..Pad::REST
            },
            1.0,
        );
        assert!(run.position[2] > walk.position[2] * 1.9);
    }

    #[test]
    fn start_asks_to_exit() {
        assert!(wants_exit(&Pad {
            buttons: BUTTON_START | BUTTON_CROSS,
            ..Pad::REST
        }));
        assert!(!wants_exit(&Pad::REST));
    }
}
