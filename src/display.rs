use std::sync::{Arc, Mutex};

use eyre::Result;
use raylib::prelude::*;

use crate::{
    IOREG_SIZE, IOREG_START_ADDRESS, IoRegister, VRAM_START_ADDRESS, VRAM_TILE_MAP1_SIZE,
    VRAM_TILE_MAP1_START_ADDRESS, VRAM_TILE_MAP2_SIZE, VRAM_TILE_MAP2_START_ADDRESS,
};

#[derive(Debug)]
pub struct Display {
    rl: RaylibHandle,
    rt: RaylibThread,
    frame_num: usize,
}

const TILE_DATA_SIZE: usize = 0x1800;
const BYTES_PER_TILE: usize = 16;
const BYTES_PER_LINE: usize = 2;
const PIXELS_PER_TILE: usize = 8;
const TILES_PER_ROW: usize = 256 / PIXELS_PER_TILE;

enum TileMapAddressingMode {
    Unsigned,
    Signed,
}

impl Display {
    pub const SCALE_FACTOR: i32 = 5;

    const PALETTE: [Color; 4] = [
        Color::WHITE,
        Color::LIGHTGRAY,
        Color::DARKGRAY,
        Color::BLACK,
    ];

    pub fn new(rl: RaylibHandle, rt: RaylibThread) -> Self {
        Self {
            rl,
            rt,
            frame_num: 0,
        }
    }

    fn get_mapped_tiles(map: &[u8], vram: &[u8], mode: TileMapAddressingMode) -> Result<Vec<Tile>> {
        let mut res = vec![];

        for index in map {
            let start = match mode {
                TileMapAddressingMode::Unsigned => {
                    usize::from(index + u8::try_from(0x8000 - VRAM_START_ADDRESS)?)
                }
                TileMapAddressingMode::Signed => usize::try_from(
                    i16::from(*index) + i16::from(i8::try_from(0x8800 - VRAM_START_ADDRESS)?),
                )?,
            } * BYTES_PER_TILE;
            let mut bytes = [0; 16];
            bytes.copy_from_slice(&vram[start..start + 16]);
            res.push(Tile { bytes })
        }

        Ok(res)
    }

    pub fn update(
        &mut self,
        vram: Arc<Mutex<Vec<u8>>>,
        ioreg: Arc<Mutex<[u8; IOREG_SIZE]>>,
    ) -> Result<()> {
        // let map1 = &vram[VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS..VRAM_TILE_MAP1_SIZE];
        // let map2 = &vram[VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS..VRAM_TILE_MAP2_SIZE];

        let tiles = {
            eprintln!("[Display] waiting for VRAM lock...");
            let vram = vram.lock().unwrap();
            eprintln!("[Display] got VRAM lock");
            let map1_start = VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS;
            let map2_start = VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS;
            let tile_maps = [
                &vram[map1_start..map1_start + VRAM_TILE_MAP1_SIZE],
                &vram[map2_start..map2_start + VRAM_TILE_MAP2_SIZE],
            ]
            .concat();
            eprintln!("vram: {:?}", &vram);
            eprintln!("tile_maps: {:?}", &tile_maps);
            let tiles = Self::get_mapped_tiles(&tile_maps, &vram, TileMapAddressingMode::Unsigned)?;
            eprintln!("tiles: {:?}", &tiles);
            tiles
        };

        let mut d = self.rl.begin_drawing(&self.rt);
        d.clear_background(Color::BLACK);
        d.draw_text(&self.frame_num.to_string(), 0, 0, 20, Color::RED);

        for (tile_idx, tile) in tiles.iter().enumerate() {
            for (line_idx, line) in tile.get_pixels().iter().enumerate() {
                let mut ioreg = ioreg.lock().unwrap();
                ioreg[IoRegister::LY as usize - IOREG_START_ADDRESS] =
                    (ioreg[IoRegister::LY as usize - IOREG_START_ADDRESS] + 1) % 153;
                for (pixel_idx, pixel) in line.iter().enumerate() {
                    let color = Self::PALETTE[usize::from(*pixel)];
                    let x = (pixel_idx + ((tile_idx % TILES_PER_ROW) * PIXELS_PER_TILE)) as i32
                        * Self::SCALE_FACTOR;
                    let y = (line_idx + ((tile_idx / TILES_PER_ROW) * PIXELS_PER_TILE)) as i32
                        * Self::SCALE_FACTOR;

                    d.draw_rectangle(x, y, Self::SCALE_FACTOR, Self::SCALE_FACTOR, color);
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
                (lsb & 0b00000001) | (msb & 0b00000001) << 1,
            ];
        }

        res
    }
}
