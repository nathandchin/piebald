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
pub const SCALE_FACTOR: f32 = 5.0;

#[derive(Debug)]
pub struct Display {
    pixels: [u8; PIXELS_PER_FULL_SCREEN_ROW * PIXELS_PER_FULL_SCREEN_COL * BYTES_PER_PIXEL],
    rend: Option<Renderer>,
}

#[derive(Debug)]
struct Renderer {
    rl: RaylibHandle,
    rt: RaylibThread,
    texture: WeakTexture2D,
}

#[allow(unused)]
#[derive(Clone, Copy, Debug)]
enum TileIdType {
    Object,
    BackgroundWindow,
}

#[derive(Clone, Copy, Debug)]
enum TileMapAddressingMode {
    Unsigned,
    Signed,
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // Not sure about this - investigate more
        unsafe {
            self.rl.unload_texture(&self.rt, self.texture.clone());
        }
    }
}

impl Renderer {
    fn new() -> Result<Self> {
        let (mut rl, thread) = raylib::init()
            .size(256 * SCALE_FACTOR as i32, 256 * SCALE_FACTOR as i32)
            .build();
        let mut image = Image::gen_image_color(
            PIXELS_PER_FULL_SCREEN_COL as i32,
            PIXELS_PER_FULL_SCREEN_ROW as i32,
            Color::WHITE,
        );
        image.set_format(PIXEL_FORMAT);
        let texture = rl.load_texture_from_image(&thread, &image)?;
        let texture = unsafe { texture.make_weak() };
        Ok(Renderer {
            rl,
            rt: thread,
            texture,
        })
    }

    fn draw(&mut self, frame: usize, pixels: &[u8]) -> Result<()> {
        let mut d = self.rl.begin_drawing(&self.rt);
        self.texture.update_texture(pixels)?;
        d.draw_texture_ex(
            &self.texture,
            Vector2::zero(),
            0.0,
            SCALE_FACTOR,
            Color::WHITE,
        );

        if cfg!(debug_assertions) {
            d.draw_text(&format!("Frame: {frame}"), 10, 10, 20, Color::RED);
        }

        Ok(())
    }
}

impl Display {
    const PALETTE: [u8; 4] = [0xff, 0x6e, 0xb0, 0x00];

    pub fn new(do_render: bool) -> Result<Self> {
        let rend = if do_render {
            Some(Renderer::new()?)
        } else {
            None
        };

        Ok(Self {
            pixels: [0; _],
            rend,
        })
    }

    pub fn draw(&mut self, frame: usize) -> Result<()> {
        match self.rend.as_mut() {
            Some(rend) => rend.draw(frame, &self.pixels),
            None => Ok(()),
        }
    }

    pub fn update_scanline(
        &mut self,
        scanline: usize,
        vram: &[u8],
        ioreg: &mut IoRegisters,
    ) -> Result<()> {
        let addressing_mode = if ioreg.get_reg(IoRegisterOffset::LCDC) & 0b00010000 == 0b00010000 {
            TileMapAddressingMode::Unsigned
        } else {
            TileMapAddressingMode::Signed
        };

        let tile_map = if ioreg.get_reg(IoRegisterOffset::LCDC) & 0b01000000 == 0b01000000 {
            &vram[VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS
                ..VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS + VRAM_TILE_MAP2_SIZE]
        } else {
            &vram[VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS
                ..VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS + VRAM_TILE_MAP1_SIZE]
        };

        // We are only concerned with the tiles that are on the current scanline
        let tile_map = {
            let start = scanline / PIXELS_PER_TILE * TILES_PER_ROW;
            let end = start + TILES_PER_ROW;
            &tile_map[start..end]
        };

        tile_map
            .iter()
            // Map list of tile indices -> list of tile structs
            .flat_map(|&tile_idx| {
                Tile::from_map_index(tile_idx, vram, TileIdType::Object, addressing_mode)
            })
            .enumerate()
            .for_each(|(tile_idx, tile)| {
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
            });

        Ok(())
    }
}

#[derive(Debug)]
struct Tile {
    bytes: [u8; BYTES_PER_TILE],
}

impl Tile {
    fn from_map_index(
        map_index: u8,
        memory: &[u8],
        tile_type: TileIdType,
        mode: TileMapAddressingMode,
    ) -> Result<Self> {
        let start = if matches!(tile_type, TileIdType::BackgroundWindow)
            && matches!(mode, TileMapAddressingMode::Signed)
        {
            usize::try_from(i16::try_from(0x9000 - VRAM_START_ADDRESS)? + i16::from(map_index))?
        } else {
            (0x8000 - VRAM_START_ADDRESS) + usize::from(map_index)
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
