use eyre::Result;
use raylib::prelude::*;

use crate::{
    IoRegisterOffset, IoRegisters, VRAM_START_ADDRESS, VRAM_TILE_MAP1_SIZE,
    VRAM_TILE_MAP1_START_ADDRESS, VRAM_TILE_MAP2_SIZE, VRAM_TILE_MAP2_START_ADDRESS,
};

// Gameboy generic
const BYTES_PER_TILE: usize = 16;
const PIXELS_PER_TILE: usize = 8;
const PIXELS_PER_FULL_SCREEN_ROW: usize = 256;
const PIXELS_PER_FULL_SCREEN_COL: usize = 256;
const TILES_PER_ROW: usize = PIXELS_PER_FULL_SCREEN_COL / PIXELS_PER_TILE;

// Specific to this implementation
const PIXEL_FORMAT: PixelFormat = PixelFormat::PIXELFORMAT_UNCOMPRESSED_GRAYSCALE;
const BYTES_PER_PIXEL: usize = 1;

#[derive(Debug)]
pub struct Display {
    rl: RaylibHandle,
    rt: RaylibThread,
    pixels: [u8; PIXELS_PER_FULL_SCREEN_ROW * PIXELS_PER_FULL_SCREEN_COL * BYTES_PER_PIXEL],
    texture: Texture2D,
}

enum TileMapAddressingMode {
    Unsigned,
    Signed,
}

impl Display {
    pub const SCALE_FACTOR: f32 = 5.0;

    const PALETTE: [u8; 4] = [0xff, 0x6e, 0xb0, 0x00];

    pub fn new(mut rl: RaylibHandle, rt: RaylibThread) -> Result<Self> {
        let mut image = Image::gen_image_color(
            PIXELS_PER_FULL_SCREEN_COL as i32,
            PIXELS_PER_FULL_SCREEN_ROW as i32,
            Color::WHITE,
        );
        image.set_format(PIXEL_FORMAT);
        let texture = rl.load_texture_from_image(&rt, &image)?;

        Ok(Self {
            rl,
            rt,
            pixels: [0; _],
            texture,
        })
    }

    pub fn draw(&mut self, frame: usize) -> Result<()> {
        let mut d = self.rl.begin_drawing(&self.rt);
        self.texture.update_texture(&self.pixels)?;
        d.draw_texture_ex(
            &self.texture,
            Vector2::zero(),
            0.0,
            Self::SCALE_FACTOR,
            Color::WHITE,
        );

        if cfg!(debug_assertions) {
            d.draw_text(&format!("Frame: {frame}"), 10, 10, 20, Color::RED);
        }

        Ok(())
    }

    pub fn update_scanline(
        &mut self,
        scanline: usize,
        vram: &[u8],
        ioreg: &mut IoRegisters,
    ) -> Result<()> {
        // TODO: dynamically choose this
        const MODE: TileMapAddressingMode = TileMapAddressingMode::Unsigned;

        let map = if ioreg.get_reg(IoRegisterOffset::LCDC) & 0b01000000 == 0b01000000 {
            &vram[VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS
                ..VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS + VRAM_TILE_MAP2_SIZE]
        } else {
            &vram[VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS
                ..VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS + VRAM_TILE_MAP1_SIZE]
        };

        let mut tiles = vec![];
        let start = scanline / PIXELS_PER_TILE * TILES_PER_ROW;
        let end = start + TILES_PER_ROW;

        let map = &map[start..end];

        // Obtain tiles from tile maps
        for &mapped_tile_index in map {
            tiles.push(Tile::from_map_index(mapped_tile_index, vram, MODE)?);
        }

        for (tile_idx, tile) in tiles.iter().enumerate() {
            let y = scanline + ((tile_idx / TILES_PER_ROW) * PIXELS_PER_TILE);
            for (pixel_idx, &pixel) in tile
                .get_line_pixels(scanline % PIXELS_PER_TILE)
                .iter()
                .enumerate()
            {
                let x = pixel_idx + ((tile_idx % TILES_PER_ROW) * PIXELS_PER_TILE);
                let color = Self::PALETTE[usize::from(pixel)];

                // This is dependent on the chosen PIXEL_FORMAT
                let idx = (x + y * PIXELS_PER_FULL_SCREEN_ROW) * BYTES_PER_PIXEL;
                self.pixels[idx] = color;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
struct Tile {
    bytes: [u8; BYTES_PER_TILE],
}

impl Tile {
    fn from_map_index(map_index: u8, memory: &[u8], mode: TileMapAddressingMode) -> Result<Self> {
        let start = match mode {
            TileMapAddressingMode::Unsigned => {
                usize::from(map_index + u8::try_from(0x8000 - VRAM_START_ADDRESS)?)
            }
            TileMapAddressingMode::Signed => usize::try_from(
                i16::from(map_index) + i16::from(i8::try_from(0x8800 - VRAM_START_ADDRESS)?),
            )?,
        } * BYTES_PER_TILE;
        let mut bytes = [0; BYTES_PER_TILE];
        bytes.copy_from_slice(&memory[start..start + BYTES_PER_TILE]);

        Ok(Self { bytes })
    }

    fn get_line_pixels(&self, line: usize) -> [u8; 8] {
        assert!(line * 2 + 1 < BYTES_PER_TILE);
        let lsb = self.bytes[line * 2];
        let msb = self.bytes[line * 2 + 1];

        [
            (lsb & 0b10000000) >> 7 | ((msb & 0b10000000) >> 7) << 1,
            (lsb & 0b01000000) >> 6 | ((msb & 0b01000000) >> 6) << 1,
            (lsb & 0b00100000) >> 5 | ((msb & 0b00100000) >> 5) << 1,
            (lsb & 0b00010000) >> 4 | ((msb & 0b00010000) >> 4) << 1,
            (lsb & 0b00001000) >> 3 | ((msb & 0b00001000) >> 3) << 1,
            (lsb & 0b00000100) >> 2 | ((msb & 0b00000100) >> 2) << 1,
            (lsb & 0b00000010) >> 1 | ((msb & 0b00000010) >> 1) << 1,
            (lsb & 0b00000001) | (msb & 0b00000001) << 1,
        ]
    }
}
