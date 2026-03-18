use raylib::prelude::*;

#[derive(Debug)]
pub struct Display {
    rl: RaylibHandle,
}

const SCALE_FACTOR: i32 = 8;

impl Display {
    pub fn new() -> Self {
        let (mut rl, thread) = raylib::init()
            .size(160 * SCALE_FACTOR, 144 * SCALE_FACTOR)
            .build();
        Self { rl }
    }

    pub fn update(&mut self, vram: &[u8]) {
        if vram.iter().sum::<u8>() > 0 {
            panic!("hoorah!");
        }
    }
}
