#![allow(unused)]

mod display;

use display::Display;

use std::{fs::File, io::Read};

use clap::Parser;
use eyre::{OptionExt, Result, eyre};
use log::{debug, trace};

#[derive(Parser, Debug)]
struct Args {
    boot_rom: String,
    rom: String,
}

fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    let mut boot_rom = [0; 256];
    File::open(&args.boot_rom)?.read_exact(&mut boot_rom);
    debug!("{:#x?}", &boot_rom);

    let rom = {
        let mut buf = vec![];
        File::open(&args.rom)?.read_to_end(&mut buf)?;
        buf
    };

    let mut dmg = SimpleDmg::new_with_bootrom(&boot_rom, &rom);
    dmg.execute()
}

#[derive(Default, Debug)]
struct RegisterFile {
    /// Instruction register
    ir: u8,
    /// Interrupt enable
    ie: u8,

    /// Accumulator
    a: u8,
    /// Flags
    f: u8,

    /// General purpose registers
    bc: u16,
    de: u16,
    hl: u16,

    /// Program counter
    pc: u16,
    /// Stack pointer
    sp: u16,
}

#[derive(Debug)]
struct SimpleDmg<'rom> {
    rf: RegisterFile,
    vram: Vec<u8>,
    ram: Vec<u8>, // stores both WRAM banks as well as HRAM
    rom: &'rom [u8],
    boot_rom: &'rom [u8],
    boot_rom_mapped: bool,
    display: Option<Display>,
}

const VRAM_START_ADDRESS: u16 = 0x8000;
const WRAM_START_ADDRESS: u16 = 0xc000;
const HRAM_START_ADDRESS: u16 = 0xff80;
const VRAM_SIZE: u16 = 0x2000; // 1 bank
const WRAM_SIZE: u16 = 0x2000; // 2 banks
const HRAM_SIZE: u16 = 0x7f;

const FLAG_ZERO: u8 = 0x80;
const FLAG_SUB: u8 = 0x40;
const FLAG_HALF_CARRY: u8 = 0x20;
const FLAG_CARRY: u8 = 0x10;

type OpcodeFn<'rom> = fn(&mut SimpleDmg<'rom>, opcode: u8) -> Result<(), eyre::ErrReport>;

impl<'rom> SimpleDmg<'rom> {
    pub fn new_with_bootrom(boot_rom: &'rom [u8], rom: &'rom [u8]) -> Self {
        let mut ram = vec![0; usize::from(WRAM_SIZE + HRAM_SIZE)];

        Self {
            rf: RegisterFile::default(),
            ram,
            vram: vec![0; usize::from(VRAM_SIZE)],
            rom,
            boot_rom,
            boot_rom_mapped: true,
            display: None,
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
                let actual_addr = address - WRAM_START_ADDRESS;
                let res = self.ram.get(usize::from(actual_addr)).copied();

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
            // I/O Registers
            0xFF00..0xFF80 => todo!(),
            // "High RAM (HRAM)"
            0xFF80..0xFFFF => todo!(),
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
                let actual_addr = usize::from(address - VRAM_START_ADDRESS);
                debug!("Write to VRAM at {address:#x} (={actual_addr:#x})");
                *self
                    .vram
                    .get_mut(actual_addr)
                    .ok_or_eyre(eyre!("Invalid write at address {address:#x}"))? = data;
                Ok(())
            }
            // 8 KiB External RAM
            0xA000..0xC000 => todo!(),
            // 8 KiB Work RAM (WRAM)
            0xC000..0xE000 => {
                let actual_addr = usize::from(address - WRAM_START_ADDRESS);
                debug!("Write to WRAM at {address:#x} (={actual_addr:#x})");
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
            // I/O Registers
            0xFF00..0xFF80 => {
                debug!("Write to I/O registers at {address:#x}");
                Ok(())
            }
            // "High RAM (HRAM)"
            0xFF80..0xFFFF => {
                let actual_addr =
                    usize::from(address) - usize::from(HRAM_START_ADDRESS) + usize::from(VRAM_SIZE);
                debug!("Write to HRAM at {address:#x} (={actual_addr:#x})");
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
            if let Some(d) = &mut self.display {
                d.update(&self.vram);
            }

            let opcode = self.read_pc_inc()?;
            debug!("pc:{:#x}, opcode:{:#x}", self.rf.pc, opcode);

            // STOP
            if opcode == 0x10 {
                break;
            }

            // Special prefix to signal the altenrate set of opcodes
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
        Some(Self::nop), Some(Self::ld_rr_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), None, None, None, None, None, None, None, None, Some(Self::inc_r8), None, Some(Self::ld_r8_imm8), None,
        // 0x10-0x1f
        Some(Self::stop), Some(Self::ld_rr_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), None, None, None, None, None, None, Some(Self::ld_a_r16mem), None, None, None, Some(Self::ld_r8_imm8), None,
        // 0x20-0x2f
        Some(Self::jr_cond_imm8), Some(Self::ld_rr_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), None, None, None, None, None, None, None, None, None, None, Some(Self::ld_r8_imm8), None,
        // 0x30-0x3f
        Some(Self::jr_cond_imm8), Some(Self::ld_rr_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), None, None, None, None, None, None, None, None, None, None, Some(Self::ld_r8_imm8), None,
        // 0x40-0x4f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x50-0x5f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x60-0x6f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x70-0x7f
        None, None, None, None, None, None, None, Some(Self::ld_r8_r8), None, None, None, None, None, None, None, None,
        // 0x80-0x8f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x90-0x9f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xa0-0xaf
        None, None, None, None, None, None, None, None, Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8), Some(Self::xor_a_r8),
        // 0xb0-0xbf
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xc0-0xcf
        None, None, None, None, None, None, None, None, None, None, None, None, None, Some(Self::call_imm16), None, None,
        // 0xd0-0xdf
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xe0-0xef
        Some(Self::ld_imm8mem_a), None, Some(Self::ld_cmem_a), None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0xf0-0xff
        None, None, None, None, None, None, None, None, None, None, Some(Self::ld_a_imm16mem), None, None, None, None, None,
    ];

    #[rustfmt::skip]
    const CB_OPCODES: [Option<OpcodeFn<'rom>>; 256] = [
        // 0x00-0x0f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        // 0x10-0x1f
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
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

    fn ld_rr_imm16(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x01 => {
                let nn = self.consume_16bit_direct()?;
                self.rf.bc = nn;
                trace!("LD BC,{nn:#x}");
            }
            0x11 => {
                let nn = self.consume_16bit_direct()?;
                self.rf.de = nn;
                trace!("LD DE,{nn:#x}");
            }
            0x21 => {
                let nn = self.consume_16bit_direct()?;
                self.rf.hl = nn;
                trace!("LD HL,{nn:#x}");
            }
            0x31 => {
                let nn = self.consume_16bit_direct()?;
                self.rf.sp = nn;
                trace!("LD SP,{nn:#x}");
            }
            _ => unreachable!(),
        };
        Ok(())
    }

    fn ld_r16mem_a(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x02 => {
                trace!("LD (BC), A");
                dbg!(self.rf.a, self.rf.bc);
                self.write(self.rf.bc, self.rf.a)?;
            }
            0x12 => {
                trace!("LD (DE), A");
                self.write(self.rf.de, self.rf.a)?;
            }
            0x22 => {
                trace!("LD (HL+), A");
                self.write(self.rf.hl, self.rf.a)?;
                self.rf.hl = self.rf.hl.wrapping_add(1);
            }
            0x32 => {
                trace!("LD (HL-), A");
                self.write(self.rf.hl, self.rf.a)?;
                self.rf.hl = self.rf.hl.wrapping_sub(1);
            }
            _ => unreachable!(),
        };
        Ok(())
    }

    fn ld_a_r16mem(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x0a => todo!(),
            0x1a => {
                trace!("LD A,(DE)");
                self.rf.a = self.read(self.rf.de)?;
            }
            0x2a => todo!(),
            0x3a => todo!(),
            _ => unreachable!(),
        }
        Ok(())
    }

    fn inc_r16(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x03 => {
                trace!("INC BC");
                self.rf.bc = self.rf.bc.wrapping_add(1);
            }
            0x13 => {
                trace!("INC DE");
                self.rf.de = self.rf.de.wrapping_add(1);
            }
            0x23 => {
                trace!("INC HL");
                self.rf.hl = self.rf.hl.wrapping_add(1);
            }
            0x33 => {
                trace!("INC SP");
                self.rf.sp = self.rf.sp.wrapping_add(1);
            }
            _ => unreachable!(),
        };
        Ok(())
    }

    fn inc_r8(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x0c => {
                trace!("INC C");
                let [b, c] = self.rf.bc.to_le_bytes();
                self.rf.bc = u16::from_le_bytes([b, c.wrapping_add(1)]);
            }
            0x13 => todo!(),
            0x23 => todo!(),
            0x33 => todo!(),
            _ => unreachable!(),
        };
        Ok(())
    }

    fn ld_r8_imm8(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x0e => {
                trace!("LD C,N");
                let n = self.read_pc_inc()?;
                self.rf.bc = u16::from_le_bytes([n, self.rf.bc.to_le_bytes()[1]])
            }
            0x1e => todo!(),
            0x2e => todo!(),
            0x3e => {
                trace!("LD A,N");
                let n = self.read_pc_inc()?;
                self.rf.a = n;
            }
            _ => unreachable!(),
        };
        Ok(())
    }

    fn jr_cond_imm8(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x20 => {
                let e = self.read_pc_inc()?.cast_signed();
                if self.rf.f & FLAG_ZERO != FLAG_ZERO {
                    self.rf.pc = u16::try_from(i32::from(self.rf.pc) + i32::from(e))?;
                }
                trace!("JR NZ,{e:#x}");
            }
            0x30 => todo!(),
            _ => unreachable!(),
        };
        Ok(())
    }

    fn stop(&mut self, opcode: u8) -> Result<()> {
        trace!("STOP");
        Err(eyre!("STOP encountered"))
    }

    fn ld_r8_r8(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x77 => {
                trace!("LD (HL),A");
                self.write(self.rf.hl, self.rf.a);
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn xor_a_r8(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0xa8 => todo!(),
            0xa9 => todo!(),
            0xaa => todo!(),
            0xab => todo!(),
            0xac => todo!(),
            0xad => todo!(),
            0xae => todo!(),
            0xaf => {
                trace!("XOR A");
                self.rf.a = 0;
                // All flags are set by this instruction
                self.rf.f = if self.rf.a == 0 { FLAG_ZERO } else { 0 };
            }
            _ => unreachable!(),
        };
        Ok(())
    }

    fn call_imm16(&mut self, _opcode: u8) -> Result<()> {
        let nn = self.consume_16bit_direct()?;
        let [pc_lsb, pc_msb] = self.rf.pc.to_le_bytes();

        self.rf.sp = self.rf.sp.wrapping_sub(1);
        self.write(self.rf.sp, pc_msb);
        self.rf.sp = self.rf.sp.wrapping_sub(1);
        self.write(self.rf.sp, pc_lsb);

        self.rf.pc = nn;

        Ok(())
    }

    fn ld_cmem_a(&mut self, opcode: u8) -> Result<()> {
        trace!("LDH (C),A");
        let [c, _] = self.rf.bc.to_le_bytes();
        let address = u16::from_le_bytes([c, 0xff]);
        self.write(address, self.rf.a)?;
        Ok(())
    }

    fn ld_imm8mem_a(&mut self, opcode: u8) -> Result<()> {
        let n = self.read_pc_inc()?;
        let address = u16::from_le_bytes([n, 0xff]);
        self.write(address, self.rf.a)?;
        trace!("LDH ({n:#x}),A");
        Ok(())
    }

    fn ld_a_imm16mem(&mut self, opcode: u8) -> Result<()> {
        let nn = self.consume_16bit_direct()?;
        let data = self.read(nn)?;
        self.rf.a = data;
        trace!("LD A,({nn:#x})");
        Ok(())
    }

    /*
     * CB prefix opcodes
     */

    fn bit_b3_r8(&mut self, opcode: u8) -> Result<()> {
        match opcode {
            0x7c => {
                trace!("BIT 7,H");
                if self.rf.hl.to_le_bytes()[1] & 0b10000000 == 0 {
                    self.rf.f |= FLAG_ZERO;
                } else {
                    self.rf.f &= !FLAG_ZERO;
                };
                self.rf.f &= !FLAG_SUB;
                self.rf.f |= FLAG_HALF_CARRY;
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
