//! Hex grid geometry. KayKit hex tiles are pointy-top along z (flat sides face ±x),
//! 2 units flat-to-flat, and are scaled by `HEX_SCALE` to match character size.
//! Rows run along x; odd rows are shifted half a hex to the right ("odd-r" offset).

use fsw_core::jsmath::{cos, js_round, sin};
use fsw_core::{Vec2, vec2};
use serde::Serialize;

/// `Math.sqrt(3)`, correctly rounded.
const SQRT_3: f64 = 1.7320508075688772;

pub const HEX_SCALE: f64 = 7.0;
pub const HEX_WIDTH: f64 = 2.0 * HEX_SCALE;
pub const HEX_ROW_PITCH: f64 = SQRT_3 * HEX_SCALE;
/// centre to corner
pub const HEX_RADIUS: f64 = (2.0 / SQRT_3) * HEX_SCALE;
/// centre to edge midpoint
pub const HEX_APOTHEM: f64 = HEX_SCALE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Hex {
    pub col: i32,
    pub row: i32,
}

pub const fn hex(col: i32, row: i32) -> Hex {
    Hex { col, row }
}

/// Edge directions 0..5 at 60° steps from +x towards +z:
/// 0 east, 1 south-east, 2 south-west, 3 west, 4 north-west, 5 north-east.
pub const DIR_COUNT: u8 = 6;
const EVEN_ROW: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1)];
const ODD_ROW: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (0, -1), (1, -1)];

pub fn neighbor(hex: Hex, dir: u8) -> Hex {
    let table = if hex.row & 1 != 0 { &ODD_ROW } else { &EVEN_ROW };
    let (dc, dr) = table[usize::from(dir % 6)];
    Hex { col: hex.col + dc, row: hex.row + dr }
}

/// Direction from a hex to an adjacent hex, or `None` if they are not neighbours.
pub fn direction_to(from: Hex, to: Hex) -> Option<u8> {
    (0..DIR_COUNT).find(|&dir| neighbor(from, dir) == to)
}

pub fn dir_vector(dir: u8) -> Vec2 {
    let angle = (std::f64::consts::PI / 3.0) * f64::from(dir);
    vec2(cos(angle), sin(angle))
}

pub fn hex_center(hex: Hex) -> Vec2 {
    vec2(HEX_WIDTH * (f64::from(hex.col) + 0.5 * f64::from(hex.row & 1)), HEX_ROW_PITCH * f64::from(hex.row))
}

pub fn hex_at(x: f64, z: f64) -> Hex {
    let q = ((SQRT_3 / 3.0) * x - z / 3.0) / HEX_RADIUS;
    let r = ((2.0 / 3.0) * z) / HEX_RADIUS;
    let mut rx = js_round(q);
    let mut rz = js_round(r);
    let ry = js_round(-q - r);
    let dx = (rx - q).abs();
    let dz = (rz - r).abs();
    let dy = (ry - (-q - r)).abs();
    if dx > dy && dx > dz {
        rx = -ry - rz;
    } else if dz > dy {
        rz = -rx - ry;
    }
    let (rx, rz) = (rx as i32, rz as i32);
    Hex { col: rx + (rz - (rz & 1)) / 2, row: rz }
}

pub fn hex_key(hex: Hex) -> String {
    format!("{},{}", hex.col, hex.row)
}

pub fn hex_distance(a: Hex, b: Hex) -> i32 {
    let aq = a.col - (a.row - (a.row & 1)) / 2;
    let bq = b.col - (b.row - (b.row & 1)) / 2;
    let dq = aq - bq;
    let dr = a.row - b.row;
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

pub fn mask_of(dirs: &[u8]) -> u8 {
    dirs.iter().fold(0, |mask, dir| mask | (1 << dir))
}

pub fn mask_has(mask: u8, dir: u8) -> bool {
    mask & (1 << dir) != 0
}

/// Rotates an edge mask so that direction k becomes k + steps.
pub fn rotate_mask(mask: u8, steps: i32) -> u8 {
    let s = steps.rem_euclid(6) as u32;
    let mask = u32::from(mask);
    (((mask << s) | (mask >> (6 - s))) & 0b11_1111) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_centres_through_hex_at() {
        for row in -2..12 {
            for col in -2..12 {
                let c = hex_center(hex(col, row));
                assert_eq!(hex_at(c.x, c.z), hex(col, row));
                assert_eq!(hex_at(c.x + 3.0, c.z - 3.0), hex(col, row));
            }
        }
    }

    #[test]
    fn places_neighbours_one_hex_width_away_in_the_edge_direction() {
        for h in [hex(3, 4), hex(3, 5)] {
            let c = hex_center(h);
            for dir in 0..DIR_COUNT {
                let n = neighbor(h, dir);
                let nc = hex_center(n);
                let v = dir_vector(dir);
                assert!((nc.x - c.x - v.x * HEX_WIDTH).abs() < 1e-5);
                assert!((nc.z - c.z - v.z * HEX_WIDTH).abs() < 1e-5);
                assert_eq!(direction_to(h, n), Some(dir));
                assert_eq!(direction_to(n, h), Some((dir + 3) % 6));
                assert_eq!(hex_distance(h, n), 1);
            }
        }
    }

    #[test]
    fn rotates_edge_masks() {
        assert_eq!(rotate_mask(mask_of(&[0, 3]), 1), mask_of(&[1, 4]));
        assert_eq!(rotate_mask(mask_of(&[5]), 2), mask_of(&[1]));
        assert_eq!(rotate_mask(mask_of(&[5]), -1), mask_of(&[4]));
    }
}
