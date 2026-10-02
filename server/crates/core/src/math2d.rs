//! Ground-plane vectors. The sim is 2D: x is east, z is south, y (up) is render-only.

use serde::{Deserialize, Serialize};

use crate::jsmath::{atan2, cos, js_hypot, js_sign, sin};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub z: f64,
}

pub const fn vec2(x: f64, z: f64) -> Vec2 {
    Vec2 { x, z }
}

pub fn dist(a: Vec2, b: Vec2) -> f64 {
    js_hypot(a.x - b.x, a.z - b.z)
}

pub fn dist_sq(a: Vec2, b: Vec2) -> f64 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    dx * dx + dz * dz
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Facing angle (radians) for a direction; 0 faces +z, matching the KayKit models.
pub fn angle_of(dx: f64, dz: f64) -> f64 {
    atan2(dx, dz)
}

pub fn facing_to(from: Vec2, to: Vec2) -> f64 {
    angle_of(to.x - from.x, to.z - from.z)
}

pub fn wrap_angle(a: f64) -> f64 {
    let tau = std::f64::consts::PI * 2.0;
    let mut r = a % tau;
    if r > std::f64::consts::PI {
        r -= tau;
    }
    if r < -std::f64::consts::PI {
        r += tau;
    }
    r
}

pub fn turn_toward(current: f64, target: f64, max_step: f64) -> f64 {
    let diff = wrap_angle(target - current);
    if diff.abs() <= max_step {
        return target;
    }
    wrap_angle(current + js_sign(diff) * max_step)
}

pub fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    a + wrap_angle(b - a) * t
}

/// Rotates a local offset by a facing angle (0 = +z forward, +x is the local right).
pub fn rotate(offset: Vec2, facing: f64) -> Vec2 {
    let s = sin(facing);
    let c = cos(facing);
    Vec2 { x: offset.x * c + offset.z * s, z: -offset.x * s + offset.z * c }
}

pub fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let abx = b.x - a.x;
    let abz = b.z - a.z;
    let len_sq = abx * abx + abz * abz;
    let t = if len_sq == 0.0 { 0.0 } else { clamp(((p.x - a.x) * abx + (p.z - a.z) * abz) / len_sq, 0.0, 1.0) };
    js_hypot(p.x - (a.x + abx * t), p.z - (a.z + abz * t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn angles() {
        assert_eq!(angle_of(0.0, 1.0), 0.0);
        assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-12);
        assert_eq!(turn_toward(0.0, 1.0, 0.25), 0.25);
        assert_eq!(turn_toward(0.0, 0.1, 0.25), 0.1);
        let r = rotate(vec2(1.0, 0.0), PI / 2.0);
        assert!(r.x.abs() < 1e-12 && (r.z + 1.0).abs() < 1e-12);
        assert_eq!(dist_to_segment(vec2(0.0, 1.0), vec2(-1.0, 0.0), vec2(1.0, 0.0)), 1.0);
    }
}
