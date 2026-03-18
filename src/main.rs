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
    ram: Vec<u8>,
    vram: Vec<u8>,
    rom: &'rom [u8],
    display: Option<Display>,
}

const RAM_START_ADDRESS: u16 = 0xc000;
const VRAM_START_ADDRESS: u16 = 0x8000;
const FLAG_ZERO: u8 = 0x80;
const FLAG_SUB: u8 = 0x40;
const FLAG_HALF_CARRY: u8 = 0x20;
const FLAG_CARRY: u8 = 0x10;

type OpcodeFn<'rom> = fn(&mut SimpleDmg<'rom>, opcode: u8) -> Result<(), eyre::ErrReport>;

impl<'rom> SimpleDmg<'rom> {
    pub fn new_with_bootrom(boot_rom: &'_ [u8; 256], rom: &'rom [u8]) -> Self {
        let mut ram = vec![0; 0x2000];
        ram[..256].clone_from_slice(boot_rom);

        Self {
            rf: RegisterFile {
                pc: RAM_START_ADDRESS,
                ..RegisterFile::default()
            },
            ram,
            vram: vec![0; 0x2000],
            rom,
            display: None,
        }
    }

    fn read(&self, address: u16) -> Result<u8> {
        // Ranges from https://gbdev.io/pandocs/Memory_Map.html
        match address {
            // 16 KiB ROM bank 00
            0x0000..0x4000 => todo!(),
            // 16 KiB ROM Bank 01–NN
            0x4000..0x8000 => todo!(),
            // 8 KiB Video RAM (VRAM)
            0x8000..0xA000 => todo!(),
            // 8 KiB External RAM
            0xA000..0xC000 => todo!(),
            // 8 KiB Work RAM (WRAM)
            0xC000..0xE000 => {
                let res = self
                    .ram
                    .get(usize::from(address - RAM_START_ADDRESS))
                    .copied();

                if let Some(res) = res {
                    debug!("Read {:#x} from WRAM at {:#x}", res, address);
                }

                res.ok_or_else(|| eyre!("Error reading from ram at address {address:#x}"))
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
                debug!("Write to VRAM at {:#x}", address);
                *self
                    .vram
                    .get_mut(usize::from(address - VRAM_START_ADDRESS))
                    .ok_or_eyre(eyre!("Invalid write at address {address:#x}"))? = data;
                Ok(())
            }
            // 8 KiB External RAM
            0xA000..0xC000 => todo!(),
            // 8 KiB Work RAM (WRAM)
            0xC000..0xE000 => {
                debug!("Write to WRAM at {:#x}", address);
                *self
                    .ram
                    .get_mut(usize::from(address - RAM_START_ADDRESS))
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
            0xFF80..0xFFFF => todo!(),
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
        Some(Self::stop), Some(Self::ld_rr_imm16), Some(Self::ld_r16mem_a), Some(Self::inc_r16), None, None, None, None, None, None, None, None, None, None, Some(Self::ld_r8_imm8), None,
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
        None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
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
                trace!("LD BC,nn");
                self.rf.bc = self.consume_16bit_direct()?;
            }
            0x11 => {
                trace!("LD DE,nn");
                self.rf.de = self.consume_16bit_direct()?;
            }
            0x21 => {
                trace!("LD HL,nn");
                self.rf.hl = self.consume_16bit_direct()?;
            }
            0x31 => {
                trace!("LD SP,nn");
                self.rf.sp = self.consume_16bit_direct()?;
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
                trace!("JR NZ,e");
                let e = self.read_pc_inc()?.cast_signed();
                if self.rf.f & FLAG_ZERO == FLAG_ZERO {
                    self.rf.pc = u16::try_from(i32::from(self.rf.pc) + i32::from(e))?;
                }
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
                trace!("XOR A,A");
                self.rf.a = 0;
                // All flags are set by this instruction
                self.rf.f = if self.rf.a == 0 { FLAG_ZERO } else { 0 };
            }
            _ => unreachable!(),
        };
        Ok(())
    }

    fn ld_imm8mem_a(&mut self, opcode: u8) -> Result<()> {
        trace!("LDH (n),A");
        let n = self.read_pc_inc()?;
        let address = u16::from_le_bytes([n, 0xff]);
        self.write(address, self.rf.a)?;
        Ok(())
    }

    fn ld_cmem_a(&mut self, opcode: u8) -> Result<()> {
        trace!("LDH (C),A");
        let [c, _] = self.rf.bc.to_le_bytes();
        let address = u16::from_le_bytes([c, 0xff]);
        self.write(address, self.rf.a)?;
        Ok(())
    }

    fn ld_a_imm16mem(&mut self, opcode: u8) -> Result<()> {
        trace!("LD A,(nn)");
        let nn = self.consume_16bit_direct()?;
        let data = self.read(nn)?;
        self.rf.a = data;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn init() {
        let _ = env_logger::builder().is_test(true).try_init();
    }

    #[test]
    fn load_direct_16bit_immediate() {
        init();

        let mut ram = vec![0; 0xffff];
        let code = [
            0x01, 0x34, 0x12, // LD BC,0x1234
            0x11, 0x78, 0x56, // LD DE,0x5678
            0x21, 0xad, 0xde, // LD HL,0xdead
            0x31, 0xef, 0xbe, // LD SP,0xbeef
            0x10, // STOP
        ];
        ram[0..code.len()].copy_from_slice(&code);
        let mut cpu = SimpleDmg {
            rf: RegisterFile {
                pc: RAM_START_ADDRESS,
                ..RegisterFile::default()
            },
            ram,
            vram: vec![],
            rom: &[],
            display: None
        };

        cpu.execute().unwrap();
        assert_eq!(cpu.rf.bc, 0x1234);
        assert_eq!(cpu.rf.de, 0x5678);
        assert_eq!(cpu.rf.hl, 0xdead);
        assert_eq!(cpu.rf.sp, 0xbeef);
    }

    #[test]
    fn load_accumulator_direct() {
        init();

        const DATA_ADDR: u16 = 0x0100;

        let mut ram = vec![0; 0xffff];
        let code = [
            0xfa, 0x02, 0xC1, // LD A,(0xC102) = load 0xbe into A
            0x10, // STOP
            0xde, 0xad, 0xbe, 0xef, // data
        ];
        ram[0..code.len()].copy_from_slice(&code);
        let code = [
            0xde, 0xad, 0xbe, 0xef, // data
        ];
        ram[usize::from(DATA_ADDR)..code.len() + usize::from(DATA_ADDR)].copy_from_slice(&code);
        let mut cpu = SimpleDmg {
            rf: RegisterFile {
                pc: RAM_START_ADDRESS,
                ..RegisterFile::default()
            },
            ram,
            vram: vec![],
            rom: &[],
            display: None
        };

        cpu.execute().unwrap();
        assert_eq!(cpu.rf.a, 0xbe);
    }

    #[test]
    fn xor_a() {
        init();

        let mut ram = vec![0; 0xffff];
        let code = [
            0xaf, // XOR A
            0x10, // STOP
        ];
        ram[0..code.len()].copy_from_slice(&code);

        let mut cpu = SimpleDmg {
            rf: RegisterFile {
                a: 0xde,
                pc: RAM_START_ADDRESS,
                ..RegisterFile::default()
            },
            ram,
            vram: vec![],
            rom: &[],
            display: None
        };

        cpu.execute().unwrap();
        assert_eq!(cpu.rf.a, 0)
    }

    #[test]
    fn load_indirect_8bit_a() {
        init();

        let mut ram = vec![0; 0xffff];
        let code = [
            // Copy a single byte
            0xfa, 0x08, 0xC0, // LD A,(0xC008) = load 0x88 into A
            0x01, 0x09, 0xC0, // LD BC,0xC009
            0x02, // LD (BC),A = write 0x88 to byte 9 (end)
            0x10, // STOP
            0x88, // source data
            0x00, // dest data
        ];
        ram[0..code.len()].copy_from_slice(&code);

        let mut cpu = SimpleDmg {
            rf: RegisterFile {
                pc: RAM_START_ADDRESS,
                ..RegisterFile::default()
            },
            ram,
            vram: vec![],
            rom: &[],
            display: None
        };

        cpu.execute().unwrap();
        assert_eq!(cpu.ram[8], cpu.ram[9]);
    }
}
