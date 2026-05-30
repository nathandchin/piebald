use std::ops::Range;

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
const PIXELS_PER_VISIBLE_ROW: usize = 160;
const PIXELS_PER_VISIBLE_COL: usize = 144;
pub const SCANLINES_PER_FRAME: usize = 154;
const TILES_PER_ROW: usize = PIXELS_PER_FULL_SCREEN_COL / PIXELS_PER_TILE;

// Specific to this implementation
const PIXEL_FORMAT: PixelFormat = PixelFormat::PIXELFORMAT_UNCOMPRESSED_GRAYSCALE;
const BYTES_PER_PIXEL: usize = 1;
pub const SCALE_FACTOR: f32 = 5.0;

#[derive(Debug)]
pub struct Display {
    background_pixels: PixelBuffer,
    window_pixels: PixelBuffer,
    rl: RaylibHandle,
    rt: RaylibThread,
    texture: WeakTexture2D,
}

type PixelBuffer = [u8; PIXELS_PER_FULL_SCREEN_ROW * PIXELS_PER_FULL_SCREEN_COL * BYTES_PER_PIXEL];
type LayerOffset = (u8, u8);

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

#[derive(Clone, Copy, Debug)]
enum GraphicsLayer {
    Background,
    Window,
}

impl Drop for Display {
    fn drop(&mut self) {
        // Not sure about this - investigate more
        unsafe {
            self.rl.unload_texture(&self.rt, self.texture.clone());
        }
    }
}

impl Display {
    const PALETTE: [u8; 4] = [0xff, 0x6e, 0xb0, 0x00];
    pub fn new() -> Result<Self> {
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
        Ok(Self {
            rl,
            rt: thread,
            background_pixels: [0; _],
            window_pixels: [0; _],
            texture,
        })
    }

    pub fn draw(&mut self, frame: usize, ioreg: &IoRegisters) -> Result<()> {
        let lcd_on = ioreg.get_reg(IoRegisterOffset::LCDC) & (1 << 7) == (1 << 7);

        let mut d = self.rl.begin_drawing(&self.rt);

        if lcd_on {
            self.texture.update_texture(&self.background_pixels)?;
        } else {
            let mut x = self.background_pixels.to_owned();
            x.fill(0xff);
            self.texture.update_texture(&x)?;
        }

        d.draw_texture_ex(
            &self.texture,
            Vector2::zero(),
            0.0,
            SCALE_FACTOR,
            Color::WHITE,
        );

        d.draw_rectangle_lines(
            0,
            0,
            PIXELS_PER_VISIBLE_ROW as i32 * SCALE_FACTOR as i32,
            PIXELS_PER_VISIBLE_COL as i32 * SCALE_FACTOR as i32,
            Color::BLACK,
        );

        if cfg!(debug_assertions) {
            d.draw_text(&format!("Frame: {frame}"), 10, 10, 20, Color::RED);
        }

        Ok(())
    }

    pub fn update_scanline(&mut self, scanline: usize, vram: &[u8], ioreg: &IoRegisters) {
        const MAP1_RANGE: Range<usize> = VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS
            ..VRAM_TILE_MAP1_START_ADDRESS - VRAM_START_ADDRESS + VRAM_TILE_MAP1_SIZE;
        const MAP2_RANGE: Range<usize> = VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS
            ..VRAM_TILE_MAP2_START_ADDRESS - VRAM_START_ADDRESS + VRAM_TILE_MAP2_SIZE;

        let addressing_mode = if ioreg.get_reg(IoRegisterOffset::LCDC) & (1 << 4) == (1 << 4) {
            TileMapAddressingMode::Unsigned
        } else {
            TileMapAddressingMode::Signed
        };

        {
            let tile_map = if ioreg.get_reg(IoRegisterOffset::LCDC) & (1 << 3) == (1 << 3) {
                &vram[MAP2_RANGE]
            } else {
                &vram[MAP1_RANGE]
            };
            let offset: LayerOffset = (
                ioreg.get_reg(IoRegisterOffset::SCX),
                ioreg.get_reg(IoRegisterOffset::SCY),
            );
            if offset.0 != 0 {
                todo!("BG scrolling with SCX");
            }

            self.update_scanline_layer(
                scanline,
                vram,
                addressing_mode,
                tile_map,
                offset,
                GraphicsLayer::Background,
            );
        }

        {
            let tile_map = if ioreg.get_reg(IoRegisterOffset::LCDC) & (1 << 6) == (1 << 6) {
                &vram[MAP2_RANGE]
            } else {
                &vram[MAP1_RANGE]
            };
            let offset: LayerOffset = (
                ioreg.get_reg(IoRegisterOffset::WX),
                ioreg.get_reg(IoRegisterOffset::WY),
            );
            if offset != (0, 0) {
                todo!("Window scrolling wtih WX and WY");
            }

            self.update_scanline_layer(
                scanline,
                vram,
                addressing_mode,
                tile_map,
                offset,
                GraphicsLayer::Window,
            );
        }
    }

    fn update_scanline_layer(
        &mut self,
        scanline: usize,
        vram: &[u8],
        addressing_mode: TileMapAddressingMode,
        tile_map: &[u8],
        offset: (u8, u8),
        layer: GraphicsLayer,
    ) {
        // If we're at the end of the frame, then we compute all the non-visible
        // scanlines for ease of understanding. This will change eventually.
        let scanlines = if scanline == PIXELS_PER_VISIBLE_COL {
            scanline..PIXELS_PER_FULL_SCREEN_COL
        } else {
            scanline..scanline + 1
        };

        for scanline in scanlines {
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
                    Tile::from_map_index(
                        tile_idx,
                        vram,
                        TileIdType::BackgroundWindow,
                        addressing_mode,
                    )
                })
                .enumerate()
                .for_each(|(tile_idx, tile)| {
                    let y = ((scanline).wrapping_sub(usize::from(offset.1)))
                        % PIXELS_PER_FULL_SCREEN_COL;
                    for (pixel_idx, &pixel) in tile
                        .get_line_pixels(scanline % PIXELS_PER_TILE)
                        .iter()
                        .enumerate()
                    {
                        let x = pixel_idx + (tile_idx * PIXELS_PER_TILE);
                        let color = Self::PALETTE[usize::from(pixel)];

                        // This is dependent on the chosen PIXEL_FORMAT
                        let idx = (x + y * PIXELS_PER_FULL_SCREEN_ROW) * BYTES_PER_PIXEL;
                        match layer {
                            GraphicsLayer::Background => self.background_pixels[idx] = color,
                            GraphicsLayer::Window => self.window_pixels[idx] = color,
                        }
                    }
                });
        }
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
        // TODO: revisit the 8800 method (signed addressing)
        let start = if matches!(mode, TileMapAddressingMode::Signed)
            && matches!(tile_type, TileIdType::BackgroundWindow)
        {
            0x9000
        } else {
            0x8000
        } - VRAM_START_ADDRESS
            + usize::from(map_index) * BYTES_PER_TILE;
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
