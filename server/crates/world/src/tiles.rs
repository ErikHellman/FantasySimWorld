//! Which hex edges each KayKit road / river / coast tile connects, measured from
//! the tile geometry (edge directions as in `hex`, tile unrotated).

use serde::Serialize;

use crate::hex::rotate_mask;

const fn mask(dirs: &[u8]) -> u8 {
    let mut mask = 0;
    let mut i = 0;
    while i < dirs.len() {
        mask |= 1 << dirs[i];
        i += 1;
    }
    mask
}

const ROAD_TILES: [(&str, u8); 13] = [
    ("hex_road_M", mask(&[3])),
    ("hex_road_A", mask(&[0, 3])),
    ("hex_road_B", mask(&[3, 5])),
    ("hex_road_C", mask(&[3, 4])),
    ("hex_road_D", mask(&[1, 3, 5])),
    ("hex_road_E", mask(&[0, 3, 5])),
    ("hex_road_F", mask(&[0, 1, 3])),
    ("hex_road_G", mask(&[2, 3, 4])),
    ("hex_road_H", mask(&[0, 2, 3, 4])),
    ("hex_road_I", mask(&[1, 2, 4, 5])),
    ("hex_road_J", mask(&[0, 1, 2, 3])),
    ("hex_road_K", mask(&[1, 2, 3, 4, 5])),
    ("hex_road_L", mask(&[0, 1, 2, 3, 4, 5])),
];

const RIVER_TILES: [(&str, u8); 12] = [
    ("hex_river_A", mask(&[0, 3])),
    ("hex_river_B", mask(&[3, 5])),
    ("hex_river_C", mask(&[3, 4])),
    ("hex_river_D", mask(&[1, 3, 5])),
    ("hex_river_E", mask(&[0, 3, 5])),
    ("hex_river_F", mask(&[0, 1, 3])),
    ("hex_river_G", mask(&[2, 3, 4])),
    ("hex_river_H", mask(&[0, 2, 3, 4])),
    ("hex_river_I", mask(&[1, 2, 4, 5])),
    ("hex_river_J", mask(&[0, 1, 2, 3])),
    ("hex_river_K", mask(&[1, 2, 3, 4, 5])),
    ("hex_river_L", mask(&[0, 1, 2, 3, 4, 5])),
];

/// Coast tiles by which edges face water.
const COAST_TILES: [(&str, u8); 4] = [
    ("hex_coast_A", mask(&[1])),
    ("hex_coast_B", mask(&[1, 2])),
    ("hex_coast_C", mask(&[0, 1, 2])),
    ("hex_coast_D", mask(&[0, 1, 2, 5])),
];

/// River runs 0-3 on both; the road crosses on 1-4 (A) or 2-5 (B).
const CROSSING_TILES: [(&str, u8, u8); 2] =
    [("hex_river_crossing_A", mask(&[0, 3]), mask(&[1, 4])), ("hex_river_crossing_B", mask(&[0, 3]), mask(&[2, 5]))];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TileChoice {
    pub model: &'static str,
    /// rotation in 60° steps: tile edge k ends up at k + rot
    pub rot: u8,
}

fn find(tiles: &[(&'static str, u8)], mask: u8) -> Option<TileChoice> {
    for &(model, base) in tiles {
        for rot in 0..6 {
            if rotate_mask(base, rot) == mask {
                return Some(TileChoice { model, rot: rot as u8 });
            }
        }
    }
    None
}

pub fn road_tile(mask: u8) -> Option<TileChoice> {
    find(&ROAD_TILES, mask)
}

pub fn river_tile(mask: u8) -> Option<TileChoice> {
    find(&RIVER_TILES, mask)
}

pub fn coast_tile(water_mask: u8) -> Option<TileChoice> {
    find(&COAST_TILES, water_mask)
}

pub fn crossing_tile(river_mask: u8, road_mask: u8) -> Option<TileChoice> {
    for &(model, river, road) in &CROSSING_TILES {
        for rot in 0..6 {
            if rotate_mask(river, rot) == river_mask && rotate_mask(road, rot) == road_mask {
                return Some(TileChoice { model, rot: rot as u8 });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::mask_of;

    #[test]
    fn finds_a_road_and_river_tile_for_every_edge_combination() {
        for mask in 1..64u8 {
            assert!(road_tile(mask).is_some(), "road mask {mask}");
            if mask.count_ones() >= 2 {
                assert!(river_tile(mask).is_some(), "river mask {mask}");
            }
        }
    }

    #[test]
    fn picks_the_crossing_tile_for_a_ne_sw_river_under_an_east_west_road() {
        assert_eq!(
            crossing_tile(mask_of(&[2, 5]), mask_of(&[0, 3])),
            Some(TileChoice { model: "hex_river_crossing_A", rot: 2 })
        );
    }
}
