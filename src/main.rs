#![allow(unused)]

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
    rom: &'rom [u8],
}

const RAM_START_ADDRESS: u16 = 0xc000;
const FLAG_ZERO: u8 = 0x80;
const FLAG_SUB: u8 = 0x40;
const FLAG_HALF_CARRY: u8 = 0x20;
const FLAG_CARRY: u8 = 0x10;

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
            rom,
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
            0xC000..0xE000 => self
                .ram
                .get(usize::from(address - RAM_START_ADDRESS))
                .copied()
                .ok_or_else(|| eyre!("Error reading from ram at address {address:#x}")),
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
                Ok(())
            }
            // 8 KiB External RAM
            0xA000..0xC000 => todo!(),
            // 8 KiB Work RAM (WRAM)
            0xC000..0xE000 => {
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
            let opcode = self.read_pc_inc()?;
            debug!("pc:{:#x}, opcode:{:#x}", self.rf.pc, opcode);

            if cb_prefix {
                cb_prefix = false;
                self.cb_operation(opcode);
                continue;
            }

            match opcode {
                0x00 => trace!("NOP"),
                0x10 => {
                    trace!("STOP");
                    break;
                }

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

                0x0e => {
                    trace!("LD C,N");
                    let n = self.read_pc_inc()?;
                    self.rf.bc = u16::from_le_bytes([n, self.rf.bc.to_le_bytes()[1]])
                }
                0x3e => {
                    trace!("LD A,N");
                    let n = self.read_pc_inc()?;
                    self.rf.a = n;
                }

                0x20 => {
                    trace!("JR NZ,e");
                    let e = self.read_pc_inc()?.cast_signed();
                    if self.rf.f & FLAG_ZERO == FLAG_ZERO {
                        self.rf.pc = u16::try_from(i32::from(self.rf.pc) + i32::from(e))?;
                    }
                }

                0xaf => {
                    trace!("XOR A,A");
                    self.rf.a = 0;
                    self.rf.f = 0 | if self.rf.a == 0 { FLAG_ZERO } else { 0 };
                }

                0xcb => {
                    cb_prefix = true;
                    continue;
                }

                0xe2 => {
                    trace!("LDH (C),A");
                    let [c, _] = self.rf.bc.to_le_bytes();
                    let address = u16::from_le_bytes([c, 0xff]);
                    self.write(address, self.rf.a)?;
                    debug!("Wrote data {:#x} to address {:#x}", self.rf.a, address);
                }

                0xfa => {
                    trace!("LD A,(nn)");
                    let nn = self.consume_16bit_direct()?;
                    debug!("nn = {nn:#x}");
                    let data = self.read(nn)?;
                    debug!("(nn) = {data:#x}");
                    self.rf.a = data;
                }

                _ => todo!("Opcode not yet implemented: {opcode:#x}"),
            };
        }

        Ok(())
    }

    fn cb_operation(&mut self, opcode: u8) {
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
            _ => todo!("CB-prefixed opcode not yet implemented: {opcode:#x}"),
        }
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
            rom: &[],
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
            rom: &[],
        };

        cpu.execute();
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
            rom: &[],
        };

        cpu.execute();
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
            rom: &[],
        };

        cpu.execute();
        assert_eq!(cpu.ram[8], cpu.ram[9]);
    }
}
