mod display;

use display::Display;

use std::{
    fs::File,
    io::Read,
    ops::Shl,
    sync::{Arc, Mutex},
};

use bitflags::bitflags;
use clap::Parser;
use eyre::{OptionExt, Result, eyre};
use log::{debug, trace};
use strum_macros::FromRepr;

#[derive(Parser, Debug)]
struct Args {
    boot_rom: String,
    rom: String,
}

fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    let mut boot_rom = [0; 256];
    File::open(&args.boot_rom)?.read_exact(&mut boot_rom)?;

    let rom = {
        let mut buf = vec![];
        File::open(&args.rom)?.read_to_end(&mut buf)?;
        buf
    };

    let mut dmg = SimpleDmg::new_with_bootrom(&boot_rom, &rom);

    let vram = Arc::clone(&dmg.vram);
    let ioreg = Arc::clone(&dmg.ioreg);

    std::thread::spawn(move || {
        let (rl, thread) = raylib::init()
            // .size(160 * Display::SCALE_FACTOR, 144 * Display::SCALE_FACTOR)
            .size(256 * Display::SCALE_FACTOR, 256 * Display::SCALE_FACTOR)
            .build();

        let mut display = Display::new(rl, thread);

        loop {
            display.update(&vram, &ioreg).unwrap();
        }
    });

    dmg.execute()
}

#[allow(unused)]
#[derive(Default, Debug)]
struct RegisterFile {
    /// Instruction register
    ir: u8,
    /// Interrupt enable
    ie: u8,

    /// Accumulator
    a: u8,
    /// Flags
    f: Flags,

    /// General purpose registers
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    h: u8,
    l: u8,

    /// Program counter
    pc: u16,
    /// Stack pointer
    sp: u16,
}

#[allow(clippy::upper_case_acronyms)]
#[repr(u16)]
#[derive(FromRepr, Copy, Clone, Debug)]
enum IoRegisterOffset {
    // Audio
    NR11 = 0xff11,
    NR12 = 0xff12,
    NR13 = 0xff13,
    NR14 = 0xff14,
    NR50 = 0xff24,
    NR51 = 0xff25,
    NR52 = 0xff26,

    // Display
    LCDC = 0xff40,
    STAT = 0xff41,
    SCY = 0xff42,
    SCX = 0xff43,
    LY = 0xff44,
    LYC = 0xff45,
    BGP = 0xff47,
    OPB0 = 0xff48,
    OPB1 = 0xff49,
    WY = 0xff4a,
    WX = 0xff4b,
    BANK = 0xff50,
}

#[derive(Clone, Debug)]
struct IoRegisters {
    dat: [u8; IOREG_SIZE],
}

impl IoRegisters {
    fn new() -> Self {
        Self {
            dat: [0; IOREG_SIZE],
        }
    }

    fn get_reg(&self, reg: IoRegisterOffset) -> u8 {
        self.dat[reg as usize - IOREG_START_ADDRESS]
    }

    fn set_reg(&mut self, reg: IoRegisterOffset, val: u8) {
        self.dat[reg as usize - IOREG_START_ADDRESS] = val;
    }
}

#[derive(Debug)]
struct SimpleDmg<'rom> {
    rf: RegisterFile,
    vram: Arc<Mutex<Vec<u8>>>,
    ram: Vec<u8>, // stores both WRAM banks as well as HRAM
    rom: &'rom [u8],
    boot_rom: &'rom [u8],
    boot_rom_mapped: bool,
    ioreg: Arc<Mutex<IoRegisters>>,
}

const VRAM_START_ADDRESS: usize = 0x8000;
const VRAM_SIZE: usize = 0x2000; // 1 bank
const VRAM_TILE_MAP1_START_ADDRESS: usize = 0x9800;
const VRAM_TILE_MAP1_SIZE: usize = 0x3ff;
const VRAM_TILE_MAP2_START_ADDRESS: usize = 0x9C00;
const VRAM_TILE_MAP2_SIZE: usize = 0x3ff;

const WRAM_START_ADDRESS: usize = 0xc000;
const WRAM_SIZE: usize = 0x2000; // 2 banks

const HRAM_START_ADDRESS: usize = 0xff80;
const HRAM_SIZE: usize = 0x80;

const IOREG_START_ADDRESS: usize = 0xff00;
const IOREG_SIZE: usize = 0x80;

bitflags! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct Flags: u8 {
        /// Carry
        const C = 0b00010000;
        /// Half Carry
        const H = 0b00100000;
        /// Subtraction
        const N = 0b01000000;
        /// Zero
        const Z = 0b10000000;

        // Declare all bits as known so that they aren't invisibly modified
        const _ = !0;
    }
}

type OpcodeFn<'rom> = fn(&mut SimpleDmg<'rom>, opcode: u8) -> Result<(), eyre::ErrReport>;

impl<'rom> SimpleDmg<'rom> {
    pub fn new_with_bootrom(boot_rom: &'rom [u8], rom: &'rom [u8]) -> Self {
        let ram = vec![0; WRAM_SIZE + HRAM_SIZE];

        Self {
            rf: RegisterFile::default(),
            ram,
            vram: Arc::new(Mutex::new(vec![0; VRAM_SIZE])),
            rom,
            boot_rom,
            boot_rom_mapped: true,
            ioreg: Arc::new(Mutex::new(IoRegisters::new())),
        }
    }

    fn get_r8(&self, r: u8) -> Result<u8> {
        Ok(match r {
            0 => self.rf.b,
            1 => self.rf.c,
            2 => self.rf.d,
            3 => self.rf.e,
            4 => self.rf.h,
            5 => self.rf.l,
            6 => {
                let addr = u16::from_be_bytes([self.rf.h, self.rf.l]);
                self.read(addr)?
            }
            7 => self.rf.a,
            _ => unreachable!("Invalid R8 identifier"),
        })
    }

    fn set_r8(&mut self, r: u8, n: u8) -> Result<()> {
        match r {
            0 => self.rf.b = n,
            1 => self.rf.c = n,
            2 => self.rf.d = n,
            3 => self.rf.e = n,
            4 => self.rf.h = n,
            5 => self.rf.l = n,
            6 => {
                let addr = u16::from_be_bytes([self.rf.h, self.rf.l]);
                self.write(addr, n)?;
            }
            7 => self.rf.a = n,
            _ => unreachable!("Invalid R8 identifier"),
        }
        Ok(())
    }

    fn get_r8_name(r: u8) -> &'static str {
        match r {
            0 => "B",
            1 => "C",
            2 => "D",
            3 => "E",
            4 => "H",
            5 => "L",
            6 => "(HL)",
            7 => "A",
            _ => unreachable!("Invalid R8 identifier"),
        }
    }

    fn get_r16(&self, r: u8) -> u16 {
        match r {
            0 => u16::from_be_bytes([self.rf.b, self.rf.c]),
            1 => u16::from_be_bytes([self.rf.d, self.rf.e]),
            2 => u16::from_be_bytes([self.rf.h, self.rf.l]),
            3 => self.rf.sp,
            _ => unreachable!("Invalid R16 identifier"),
        }
    }

    fn set_r16(&mut self, r: u8, n: u16) {
        let [lsb, msb] = n.to_le_bytes();
        match r {
            0 => {
                self.rf.b = msb;
                self.rf.c = lsb;
            }
            1 => {
                self.rf.d = msb;
                self.rf.e = lsb;
            }
            2 => {
                self.rf.h = msb;
                self.rf.l = lsb;
            }
            3 => {
                self.rf.sp = n;
            }
            _ => unreachable!("Invalid R16 identifier"),
        };
    }

    fn get_r16_name(r: u8) -> &'static str {
        match r {
            0 => "BC",
            1 => "DE",
            2 => "HL",
            3 => "SP",
            _ => unreachable!("Invalid R16 identifier"),
        }
    }

    fn get_r16stk(&self, r: u8) -> u16 {
        match r {
            0 => u16::from_be_bytes([self.rf.b, self.rf.c]),
            1 => u16::from_be_bytes([self.rf.d, self.rf.e]),
            2 => u16::from_be_bytes([self.rf.h, self.rf.l]),
            3 => u16::from_be_bytes([self.rf.a, self.rf.f.bits()]),
            _ => unreachable!("Invalid R16stk identifier"),
        }
    }

    fn set_r16stk(&mut self, r: u8, n: u16) {
        let [lsb, msb] = n.to_le_bytes();
        match r {
            0..=2 => {
                self.set_r16(r, n);
            }
            3 => {
                self.rf.a = msb;
                self.rf.f = Flags::from_bits_retain(lsb);
            }
            _ => unreachable!("Invalid R16stk identifier"),
        };
    }

    fn get_r16stk_name(r: u8) -> &'static str {
        match r {
            0 => "BC",
            1 => "DE",
            2 => "HL",
            3 => "AF",
            _ => unreachable!("Invalid R16stk identifier"),
        }
    }

    fn get_r16mem(&mut self, r: u8) -> Result<u8> {
        match r {
            0 | 1 => self.read(self.get_r16(r)),
            2 | 3 => {
                let mut hl = u16::from_be_bytes([self.rf.h, self.rf.l]);

                // Unwrap now so that we don't increment/decrement HL if there
                // was an error
                let res = self.read(hl)?;

                // Handle HL+/HL-
                if r == 2 {
                    hl = hl.wrapping_add(1);
                } else {
                    hl = hl.wrapping_sub(1);
                }
                let [h, l] = hl.to_be_bytes();
                self.rf.h = h;
                self.rf.l = l;

                Ok(res)
            }
            _ => unreachable!("Invalid R16mem identifier"),
        }
    }

    fn set_r16mem(&mut self, r: u8, n: u8) -> Result<()> {
        match r {
            0 | 1 => self.write(self.get_r16(r), n),
            2 | 3 => {
                let mut hl = u16::from_be_bytes([self.rf.h, self.rf.l]);

                self.write(hl, n)?;

                // Handle HL+/HL-
                if r == 2 {
                    hl = hl.wrapping_add(1);
                } else {
                    hl = hl.wrapping_sub(1);
                }
                let [h, l] = hl.to_be_bytes();
                self.rf.h = h;
                self.rf.l = l;

                Ok(())
            }
            _ => unreachable!("Invalid R16mem identifier"),
        }
    }

    fn get_r16mem_name(r: u8) -> &'static str {
        match r {
            0 => "BC",
            1 => "DE",
            2 => "HL+",
            3 => "HL-",
            _ => unreachable!("Invalid R16mem identifier"),
        }
    }

    fn read(&self, address: u16) -> Result<u8> {
        // Ranges from https://gbdev.io/pandocs/Memory_Map.html
        match address {
            // 16 KiB ROM bank 00
            0x0000..0x4000 => {
                let actual_addr = usize::from(address);

                let res = if self.boot_rom_mapped && actual_addr < 0x100 {
                    self.boot_rom.get(actual_addr).copied()
                } else {
                    self.rom.get(actual_addr).copied()
                };

                if let Some(res) = res {
                    debug!(
                        "Read {:#x} from ROM at {:#x} (={actual_addr:#x})",
                        res, address
                    );
                }
                res.ok_or_else(|| eyre!("Error reading from ROM at address {address:#x}"))
            }
            // 16 KiB ROM Bank 01–NN
            0x4000..0x8000 => todo!(),
            // 8 KiB Video RAM (VRAM)
            0x8000..0xA000 => todo!(),
            // 8 KiB External RAM
            0xA000..0xC000 => todo!(),
            // 8 KiB Work RAM (WRAM)
            0xC000..0xE000 => {
                let actual_addr = usize::from(address) - WRAM_START_ADDRESS;
                let res = self.ram.get(actual_addr).copied();

                if let Some(res) = res {
                    debug!(
                        "Read {:#x} from WRAM at {:#x} (={actual_addr:#x})",
                        res, address
                    );
                }

                res.ok_or_else(|| eyre!("Error reading from RAM at address {address:#x}"))
            }
            // Echo RAM (mirror of C000–DDFF)
            0xE000..0xFE00 => Err(eyre!("Invalid read at address {address:#x}")),
            // Object attribute memory (OAM)
            0xFE00..0xFEA0 => todo!(),
            // Not Usable
            0xFEA0..0xFF00 => Err(eyre!("Invalid read at address {address:#x}")),
            // IO Registers
            0xFF00..0xFF80 => {
                if let Some(reg) = IoRegisterOffset::from_repr(address) {
                    let res = self.ioreg.lock().unwrap().get_reg(reg);
                    debug!("Read {res:#x} from IO register at {address:#x}");
                    Ok(res)
                } else {
                    Err(eyre!("Unimplemented IO register: {address:#x}"))
                }
            }
            // "High RAM (HRAM)"
            0xFF80..0xFFFF => {
                let actual_addr = usize::from(address) - HRAM_START_ADDRESS + VRAM_SIZE;
                let res = self.ram.get(actual_addr).copied();

                if let Some(res) = res {
                    debug!(
                        "Read {:#x} from HRAM at {:#x} (={actual_addr:#x})",
                        res, address
                    );
                }

                res.ok_or_else(|| eyre!("Error reading from RAM at address {address:#x}"))
            }
            // Interrupt Enable register (IE)
            0xFFFF => todo!(),
        }
    }

    fn read_inc(&mut self, address: u16) -> Result<u8> {
        // Increment after reading so that if the read fails, we have a correct
        // PC for debugging.
        let res = self.read(address);
        self.rf.pc = self.rf.pc.wrapping_add(1);
        res
    }

    fn read_pc_inc(&mut self) -> Result<u8> {
        self.read_inc(self.rf.pc)
    }

    fn consume_16bit_direct(&mut self) -> Result<u16> {
        let nn_lsb = self.read_pc_inc()?;
        let nn_msb = self.read_pc_inc()?;
        Ok(u16::from_le_bytes([nn_lsb, nn_msb]))
    }

    fn write(&mut self, address: u16, data: u8) -> Result<()> {
        // Ranges from https://gbdev.io/pandocs/Memory_Map.html
        match address {
            // 16 KiB ROM bank 00
            0x0000..0x4000 => todo!(),
            // 16 KiB ROM Bank 01–NN
            0x4000..0x8000 => todo!(),
            // 8 KiB Video RAM (VRAM)
            0x8000..0xA000 => {
                let actual_addr = usize::from(address) - VRAM_START_ADDRESS;
                debug!("Write {data:#x} to VRAM at {address:#x} (={actual_addr:#x})");
                let x = self.vram.lock();
                match x {
                    Ok(mut vram) => {
                        *vram
                            .get_mut(actual_addr)
                            .ok_or_eyre(eyre!("Invalid write at address {address:#x}"))? = data;
                        Ok(())
                    }
                    Err(_) => Err(eyre!("VRAM lock was poisoned")),
                }
            }
            // 8 KiB External RAM
            0xA000..0xC000 => todo!(),
            // 8 KiB Work RAM (WRAM)
            0xC000..0xE000 => {
                let actual_addr = usize::from(address) - WRAM_START_ADDRESS;
                debug!("Write {data:#x} to WRAM at {address:#x} (={actual_addr:#x})");
                *self
                    .ram
                    .get_mut(actual_addr)
                    .ok_or_eyre(eyre!("Invalid write at address {address:#x}"))? = data;
                Ok(())
            }
            // Echo RAM (mirror of C000–DDFF)
            0xE000..0xFE00 => Err(eyre!("Invalid write at address {address:#x}")),
            // Object attribute memory (OAM)
            0xFE00..0xFEA0 => todo!(),
            // Not Usable
            0xFEA0..0xFF00 => Err(eyre!("Invalid write at address {address:#x}")),
            // IO Registers
            0xFF00..0xFF80 => {
                if let Some(reg) = IoRegisterOffset::from_repr(address) {
                    debug!("Write {data:#x} to IO register at {address:#x}");
                    self.ioreg.lock().unwrap().set_reg(reg, data);
                    Ok(())
                } else {
                    Err(eyre!("Unimplemented IO register: {address:#x}"))
                }
            }
            // "High RAM (HRAM)"
            0xFF80..0xFFFF => {
                let actual_addr = usize::from(address) - HRAM_START_ADDRESS + VRAM_SIZE;
                debug!("Write {data:#x} to HRAM at {address:#x} (={actual_addr:#x})");
                *self
                    .ram
                    .get_mut(actual_addr)
                    .ok_or_eyre(eyre!("Invalid write at address {address:#x}"))? = data;
                Ok(())
            }
            // Interrupt Enable register (IE)
            0xFFFF => todo!(),
        }
    }

    fn execute(&mut self) -> Result<()> {
        let mut cb_prefix = false;
        loop {
            let opcode = self.read_pc_inc()?;
            debug!("pc:{:#x}, opcode:{:#x}", self.rf.pc, opcode);

            // STOP
            if opcode == 0x10 {
                break;
            }

            // Special prefix to signal the alternate set of opcodes
            if opcode == 0xcb {
                cb_prefix = true;
                continue;
            }
            if cb_prefix {
                cb_prefix = false;
                match Self::CB_OPCODES[usize::from(opcode)] {
                    Some(f) => f(self, opcode)?,
                    None => todo!("CB-prefixed opcode not yet implemented: {opcode:#x}"),
                };
                continue;
            }

            match Self::OPCODES[usize::from(opcode)] {
                Some(f) => f(self, opcode)?,
                None => todo!("Opcode not yet implemented: {opcode:#x}"),
            };
        }
        Ok(())
    }

    #[rustfmt::skip]
    const OPCODES: [Option<OpcodeFn<'rom>>; 256] = [
        // 0x00-0x0f
        Some(Self::nop), Some(Self::ld_r16_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), Some(Self::inc_r8), Some(Self::dec_r8), Some(Self::ld_r8_imm8), None, None, None, None, None, Some(Self::inc_r8), Some(Self::dec_r8), Some(Self::ld_r8_imm8), None,
        // 0x10-0x1f
        Some(Self::stop), Some(Self::ld_r16_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), Some(Self::inc_r8), Some(Self::dec_r8), Some(Self::ld_r8_imm8), Some(Self::rla), Some(Self::jr_imm8), None, Some(Self::ld_a_r16mem), None, None, Some(Self::dec_r8), Some(Self::ld_r8_imm8), None,
        // 0x20-0x2f
        Some(Self::jr_cond_imm8), Some(Self::ld_r16_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), Some(Self::inc_r8), Some(Self::dec_r8), None, None, Some(Self::jr_cond_imm8), None, None, None, None, Some(Self::dec_r8), Some(Self::ld_r8_imm8), None,
        // 0x30-0x3f
        Some(Self::jr_cond_imm8), Some(Self::ld_r16_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), Some(Self::inc_r8), Some(Self::dec_r8), None, None, Some(Self::jr_cond_imm8), None, None, None, None, Some(Self::dec_r8), Some(Self::ld_r8_imm8), None,
        // 0x40-0x4f
        Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8),
        // 0x50-0x5f
        Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8),
        // 0x60-0x6f
        Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8),
        // 0x70-0x7f
        Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), None, Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8), Some(Self::ld_r8_r8),
        // 0x80-0x8f
        Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8), Some(Self::add_a_r8),
        // 0x90-0x9f
        Some(Self::sub_a_r8), Some(Self::sub_a_r8), Some(Self::sub_a_r8), Some(Self::sub_a_r8), Some(Self::sub_a_r8), Some(Self::sub_a_r8), Some(Self::sub_a_r8), Some(Self::sub_a_r8), None, None, None, None, None, None, None, None,
        // 0xa0-0xaf
        None, None, None, None, None, None, None, None, Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8),
        // 0xb0-0xbf
        None, None, None, None, None, None, None, None, Some(Self::cp_a_r8), Some(Self::cp_a_r8), Some(Self::cp_a_r8), Some(Self::cp_a_r8), Some(Self::cp_a_r8), Some(Self::cp_a_r8), Some(Self::cp_a_r8), Some(Self::cp_a_r8),
        // 0xc0-0xcf
        None, Some(Self::pop_r16stk), None, Some(Self::jp_imm16), None, Some(Self::push_r16stk), None, None, None, Some(Self::ret), None, None, None, Some(Self::call_imm16), None, None,
        // 0xd0-0xdf
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xe0-0xef
        Some(Self::ldh_imm8mem_a), None, Some(Self::ldh_cmem_a), None, None, None, None, None, None, None, Some(Self::ld_imm16mem_a), None, None, None, None, None,
        // 0xf0-0xff
        Some(Self::ldh_a_cmem), None, None, None, None, None, None, None, None, None, Some(Self::ld_a_imm16mem), None, None, None, Some(Self::cp_a_imm8), None,
    ];

    #[rustfmt::skip]
    const CB_OPCODES: [Option<OpcodeFn<'rom>>; 256] = [
        // 0x00-0x0f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x10-0x1f
        None, Some(Self::rl_r8), None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x20-0x2f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x30-0x3f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x40-0x4f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x50-0x5f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x60-0x6f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x70-0x7f
        None, None, None, None, None, None, None, None, None, None, None, None, Some(Self::bit_b3_r8), None, None, None,
        // 0x80-0x8f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x90-0x9f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xa0-0xaf
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xb0-0xbf
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xc0-0xcf
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xd0-0xdf
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xe0-0xef
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xf0-0xff
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
    ];

    /*
     * The ordering of the opcode functions is inspired by
     * https://gbdev.io/pandocs/CPU_Instruction_Set.html
     */

    fn nop(&mut self, _opcode: u8) -> Result<()> {
        trace!("NOP");
        Ok(())
    }

    fn ld_r16_imm16(&mut self, opcode: u8) -> Result<()> {
        let nn = self.consume_16bit_direct()?;
        trace!("LD {},{nn:#x}", Self::get_r16_name(opcode >> 4));
        self.set_r16(opcode >> 4, nn);
        Ok(())
    }

    fn ld_r16mem_a(&mut self, opcode: u8) -> Result<()> {
        trace!("LD ({}), A", Self::get_r16mem_name(opcode >> 4));
        self.set_r16mem(opcode >> 4, self.rf.a)
    }

    fn ld_a_r16mem(&mut self, opcode: u8) -> Result<()> {
        trace!("LD A, ({})", Self::get_r16mem_name(opcode >> 4));
        self.rf.a = self.get_r16mem(opcode >> 4)?;
        Ok(())
    }

    fn inc_r16(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode >> 4;
        trace!("INC {}", Self::get_r16_name(reg));
        self.set_r16(reg, self.get_r16(reg).wrapping_add(1));
        Ok(())
    }

    fn inc_r8(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode >> 3;
        trace!("INC {}", Self::get_r8_name(reg));

        let n = self.get_r8(reg)?;
        let new_n = n.wrapping_add(1);
        self.set_r8(reg, new_n)?;

        self.rf.f.set(Flags::H, ((n ^ 1 ^ new_n) & 0x10) == 0x10);
        self.rf.f.remove(Flags::N);
        self.rf.f.set(Flags::Z, new_n == 0);

        Ok(())
    }

    fn dec_r8(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode >> 3;
        trace!("DEC {}", Self::get_r8_name(reg));

        let n = self.get_r8(reg)?;
        let new_n = n.wrapping_sub(1);
        self.set_r8(reg, new_n)?;

        self.rf
            .f
            .set(Flags::H, (((n & 0xf).wrapping_sub(1 & 0xf)) & 0x10) == 0x10);
        self.rf.f.remove(Flags::N);
        self.rf.f.set(Flags::Z, new_n == 0);

        Ok(())
    }

    fn ld_r8_imm8(&mut self, opcode: u8) -> Result<()> {
        let n = self.read_pc_inc()?;
        let reg = opcode >> 3;
        trace!("LD {},{n:#x}", Self::get_r8_name(opcode >> 3));
        self.set_r8(reg, n)
    }

    fn rla(&mut self, _opcode: u8) -> Result<()> {
        trace!("RLA");
        let mut a = self.rf.a;

        let a7 = (a & 0b10000000) >> 7;
        let carry_bit: u8 = if self.rf.f.contains(Flags::C) { 1 } else { 0 };
        a = a.shl(1) | carry_bit;

        self.rf.f.remove(Flags::Z);
        self.rf.f.remove(Flags::N);
        self.rf.f.remove(Flags::H);
        self.rf.f.set(Flags::C, a7 == 1);

        self.rf.a = a;

        Ok(())
    }

    fn jr_imm8(&mut self, _opcode: u8) -> Result<()> {
        let e = self.read_pc_inc()?.cast_signed();
        trace!("JR {e:#x}");
        self.rf.pc = self.rf.pc.wrapping_add_signed(i16::from(e));
        Ok(())
    }

    fn jr_cond_imm8(&mut self, opcode: u8) -> Result<()> {
        let e = self.read_pc_inc()?.cast_signed();

        trace!(
            "JR {},{e:#x}",
            match (opcode & !0x20) >> 3 {
                0 => "NZ",
                1 => "Z",
                2 => "NC",
                3 => "C",
                _ => unreachable!(),
            }
        );

        let cond = match (opcode & !0x20) >> 3 {
            0 => !self.rf.f.contains(Flags::Z),
            1 => self.rf.f.contains(Flags::Z),
            2 => !self.rf.f.contains(Flags::C),
            3 => self.rf.f.contains(Flags::C),
            _ => unreachable!(),
        };
        if cond {
            self.rf.pc = self.rf.pc.wrapping_add_signed(i16::from(e));
        }
        Ok(())
    }

    fn stop(&mut self, _opcode: u8) -> Result<()> {
        trace!("STOP");
        Err(eyre!("STOP encountered"))
    }

    fn ld_r8_r8(&mut self, opcode: u8) -> Result<()> {
        let r_dst = opcode << 2 >> 5;
        let r_src = opcode & 0x7;
        trace!(
            "LD {},{}",
            Self::get_r8_name(r_dst),
            Self::get_r8_name(r_src)
        );
        self.set_r8(r_dst, self.get_r8(r_src)?)
    }

    fn add_a_r8(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode & 0x7;
        trace!("ADD A,{}", Self::get_r8_name(reg));
        let rhs = self.get_r8(reg)?;
        let (result, carry) = self.rf.a.overflowing_add(rhs);

        self.rf.a = result;

        self.rf.f.set(Flags::Z, result == 0);
        self.rf.f.remove(Flags::N);
        self.rf.f.set(
            Flags::H,
            (((self.rf.a & 0xf).wrapping_add(rhs & 0xf)) & 0x10) == 0x10,
        );
        self.rf.f.set(Flags::C, carry);

        Ok(())
    }

    fn sub_a_r8(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode & 0x7;
        trace!("SUB A,{}", Self::get_r8_name(reg));
        let rhs = self.get_r8(reg)?;
        let (result, carry) = self.rf.a.overflowing_sub(rhs);

        self.rf.a = result;

        self.rf.f.set(Flags::Z, result == 0);
        self.rf.f.insert(Flags::N);
        self.rf.f.set(
            Flags::H,
            (((self.rf.a & 0xf).wrapping_sub(rhs & 0xf)) & 0x10) == 0x10,
        );
        self.rf.f.set(Flags::C, carry);

        Ok(())
    }

    fn cp_a_r8(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode & 0x7;
        trace!("CP {}", Self::get_r8_name(reg));
        let rhs = self.get_r8(reg)?;
        let (result, carry) = self.rf.a.overflowing_sub(rhs);

        self.rf.f.set(Flags::Z, result == 0);
        self.rf.f.insert(Flags::N);
        self.rf.f.set(
            Flags::H,
            (((self.rf.a & 0xf).wrapping_sub(rhs & 0xf)) & 0x10) == 0x10,
        );
        self.rf.f.set(Flags::C, carry);

        Ok(())
    }

    fn xor_a_r8(&mut self, opcode: u8) -> Result<()> {
        trace!("XOR {}", Self::get_r8_name(opcode << 5 >> 5));
        self.rf.a ^= self.get_r8(opcode << 5 >> 5)?;
        self.rf.f.remove(Flags::C);
        self.rf.f.remove(Flags::H);
        self.rf.f.remove(Flags::N);
        self.rf.f.set(Flags::Z, self.rf.a == 0);
        Ok(())
    }

    fn cp_a_imm8(&mut self, _opcode: u8) -> Result<()> {
        let n = self.read_pc_inc()?;
        trace!("CP {n:#x}");
        let (result, carry) = self.rf.a.overflowing_sub(n);

        self.rf.f.set(Flags::Z, result == 0);
        self.rf.f.insert(Flags::N);
        self.rf.f.set(
            Flags::H,
            (((self.rf.a & 0xf).wrapping_sub(n & 0xf)) & 0x10) == 0x10,
        );
        self.rf.f.set(Flags::C, carry);

        Ok(())
    }

    fn ret(&mut self, _opcode: u8) -> Result<()> {
        trace!("RET");
        let nn_lsb = self.read(self.rf.sp)?;
        self.rf.sp = self.rf.sp.wrapping_add(1);
        let nn_msb = self.read(self.rf.sp)?;
        self.rf.sp = self.rf.sp.wrapping_add(1);

        self.rf.pc = u16::from_le_bytes([nn_lsb, nn_msb]);
        Ok(())
    }

    fn jp_imm16(&mut self, _opcode: u8) -> Result<()> {
        let nn = self.consume_16bit_direct()?;
            trace!("JP {nn:#x}");
        self.rf.pc = nn;
        Ok(())
    }

    fn call_imm16(&mut self, _opcode: u8) -> Result<()> {
        let nn = self.consume_16bit_direct()?;
        trace!("CALL {nn:#x}");
        let [pc_lsb, pc_msb] = self.rf.pc.to_le_bytes();

        self.rf.sp = self.rf.sp.wrapping_sub(1);
        self.write(self.rf.sp, pc_msb)?;
        self.rf.sp = self.rf.sp.wrapping_sub(1);
        self.write(self.rf.sp, pc_lsb)?;

        self.rf.pc = nn;

        Ok(())
    }

    fn pop_r16stk(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode << 2 >> 5;
        trace!("POP {}", Self::get_r16stk_name(reg));

        let lsb = self.read(self.rf.sp)?;
        self.rf.sp = self.rf.sp.wrapping_add(1);
        let msb = self.read(self.rf.sp)?;
        self.rf.sp = self.rf.sp.wrapping_add(1);

        self.set_r16stk(reg, u16::from_be_bytes([msb, lsb]));

        Ok(())
    }

    fn push_r16stk(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode << 2 >> 5;
        trace!("PUSH {}", Self::get_r16stk_name(reg));

        let [r_msb, r_lsb] = self.get_r16stk(reg).to_be_bytes();
        self.rf.sp = self.rf.sp.wrapping_sub(1);
        self.write(self.rf.sp, r_msb)?;
        self.rf.sp = self.rf.sp.wrapping_sub(1);
        self.write(self.rf.sp, r_lsb)
    }

    fn ldh_cmem_a(&mut self, _opcode: u8) -> Result<()> {
        trace!("LDH (C),A");
        let address = u16::from_be_bytes([0xff, self.rf.c]);
        self.write(address, self.rf.a)
    }

    fn ldh_imm8mem_a(&mut self, _opcode: u8) -> Result<()> {
        let n = self.read_pc_inc()?;
        trace!("LDH ({n:#x}),A");
        let address = u16::from_be_bytes([0xff, n]);
        self.write(address, self.rf.a)
    }

    fn ld_imm16mem_a(&mut self, _opcode: u8) -> Result<()> {
        let nn = self.consume_16bit_direct()?;
        trace!("LD ({nn:#x}),A");
        self.write(nn, self.rf.a)
    }

    fn ldh_a_cmem(&mut self, _opcode: u8) -> Result<()> {
        let n = self.read_pc_inc()?;
        trace!("LDH A,({n:#x})");
        self.rf.a = self.read(u16::from_be_bytes([0xff, n]))?;
        Ok(())
    }

    fn ld_a_imm16mem(&mut self, _opcode: u8) -> Result<()> {
        let nn = self.consume_16bit_direct()?;
        trace!("LD A,({nn:#x})");
        self.rf.a = self.read(nn)?;
        Ok(())
    }

    /*
     * CB prefix opcodes
     */

    fn rl_r8(&mut self, opcode: u8) -> Result<()> {
        let reg = opcode & !0x10;
        trace!("RL {}", Self::get_r8_name(reg));

        let mut curr = self.get_r8(reg)?;
        let curr7 = (curr & 0b10000000) >> 7;
        let carry_bit: u8 = if self.rf.f.contains(Flags::C) { 1 } else { 0 };
        curr = curr.shl(1) | carry_bit;

        self.rf.f.set(Flags::Z, curr == 0);
        self.rf.f.remove(Flags::N);
        self.rf.f.remove(Flags::H);
        self.rf.f.set(Flags::C, curr7 == 1);

        self.set_r8(reg, curr)
    }

    fn bit_b3_r8(&mut self, opcode: u8) -> Result<()> {
        let bit = opcode << 2 >> 5;
        let reg = opcode & !0xf8;

        trace!("BIT {bit},{}", Self::get_r8_name(reg));
        if self.get_r8(reg)? & (1 << bit) == 0 {
            self.rf.f.insert(Flags::Z);
        } else {
            self.rf.f.remove(Flags::Z);
        }

        self.rf.f.remove(Flags::N);
        self.rf.f.insert(Flags::H);
        Ok(())
    }
}
