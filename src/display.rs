use eyre::Result;
use raylib::prelude::*;

use crate::{
    VRAM_START_ADDRESS, VRAM_TILE_MAP1_SIZE, VRAM_TILE_MAP1_START_ADDRESS, VRAM_TILE_MAP2_SIZE,
    VRAM_TILE_MAP2_START_ADDRESS,
};

#[derive(Debug)]
pub struct Display {
    rl: RaylibHandle,
    rt: RaylibThread,
}

const TILE_DATA_SIZE: usize = 0x1800;
const BYTES_PER_TILE: usize = 16;
const BYTES_PER_LINE: usize = 2;
const PIXELS_PER_TILE: usize = 8;

enum TileMapAddressingMode {
    Unsigned,
    Signed,
}

impl Display {
    pub const SCALE_FACTOR: i32 = 20;

    const PALETTE: [Color; 4] = [
        Color::WHITE,
        Color::LIGHTGRAY,
        Color::DARKGRAY,
        Color::BLACK,
    ];

    pub fn new(rl: RaylibHandle, rt: RaylibThread) -> Self {
        Self { rl, rt }
    }

    fn get_mapped_tiles<'a>(
        map: &[u8],
        vram: &[u8],
        mode: TileMapAddressingMode,
    ) -> Result<Vec<Tile>> {
        let mut res = vec![];

        for index in map {
            let start = match mode {
                TileMapAddressingMode::Unsigned => {
                    usize::from(index + u8::try_from(0x8000 - VRAM_START_ADDRESS)?)
                }
                TileMapAddressingMode::Signed => usize::try_from(
                    i16::from(*index) + i16::from(i8::try_from(0x8800 - VRAM_START_ADDRESS)?),
                )?,
            };
            let mut bytes = [0; 16];
            bytes.copy_from_slice(&vram[start..start + 16]);
            res.push(Tile { bytes })
        }

        Ok(res)
    }

    pub fn update(&mut self, vram: &[u8]) -> Result<()> {
        // let map1 = &vram[VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS..VRAM_TILE_MAP1_SIZE];
        // let map2 = &vram[VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS..VRAM_TILE_MAP2_SIZE];

        let tiles = Self::get_mapped_tiles(&[0, 0], &vram, TileMapAddressingMode::Unsigned)?;

        for (tile_idx, tile) in tiles.iter().enumerate() {
            // for b in tile.bytes {
            //     eprint!("{:#x} ", b);
            // }
            // eprintln!("\n");

            // let pixels = tile.get_pixels();
            // for l in pixels {
            //     for b in l {
            //         eprint!("{:#x} ", b);
            //     }
            //     eprintln!();
            // }
            // eprintln!();

            for (line_idx, line) in tile.get_pixels().iter().enumerate() {
                for (pixel_idx, pixel) in line.iter().enumerate() {
                    let color = Self::PALETTE[usize::from(*pixel)];
                    let x = (pixel_idx + (tile_idx * PIXELS_PER_TILE)) as i32 * Self::SCALE_FACTOR;
                    let y = (line_idx + (tile_idx * PIXELS_PER_TILE)) as i32 * Self::SCALE_FACTOR;

                    self.rl.draw(&self.rt, |mut d| {
                        d.draw_rectangle(x, y, Self::SCALE_FACTOR, Self::SCALE_FACTOR, color);
                    });
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
struct Tile {
    bytes: [u8; 16],
}

impl Tile {
    fn get_pixels(&self) -> [[u8; 8]; 8] {
        let mut res = [[0; _]; _];

        for (i, line) in self.bytes.chunks_exact(BYTES_PER_LINE).enumerate() {
            let lsb = line[0];
            let msb = line[1];
            res[i] = [
                (lsb & 0b10000000) >> 7 | ((msb & 0b10000000) >> 7) << 1,
                (lsb & 0b01000000) >> 6 | ((msb & 0b01000000) >> 6) << 1,
                (lsb & 0b00100000) >> 5 | ((msb & 0b00100000) >> 5) << 1,
                (lsb & 0b00010000) >> 4 | ((msb & 0b00010000) >> 4) << 1,
                (lsb & 0b00001000) >> 3 | ((msb & 0b00001000) >> 3) << 1,
                (lsb & 0b00000100) >> 2 | ((msb & 0b00000100) >> 2) << 1,
                (lsb & 0b00000010) >> 1 | ((msb & 0b00000010) >> 1) << 1,
                (lsb & 0b00000001) >> 0 | ((msb & 0b00000001) >> 0) << 1,
            ];
        }

        res
    }
}
