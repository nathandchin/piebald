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

    let boot_rom = {
        let mut buf = vec![];
        if File::open(&args.boot_rom)?.read_to_end(&mut buf)? != 256 {
            return Err(eyre!("Boot ROM must be 256 bytes"));
        }
        buf
    };
    dbg!(boot_rom.len());
    let rom = {
        let mut buf = vec![];
        File::open(&args.rom)?.read_to_end(&mut buf)?;
        buf
    };

    todo!();

    Ok(())
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
struct SimpleDmg {
    rf: RegisterFile,
    text: [u8; 0xffff],
}

impl SimpleDmg {
    pub fn new(boot_rom: &[u8; 256]) -> Self {
        let text = [0u8; 0xffff];
        Self {
            rf: RegisterFile::default(),
            text,
        }
    }

    fn read(&self, address: u16) -> Result<u8> {
        self.text
            .get(address as usize)
            .copied()
            .ok_or_else(|| eyre!("Invalid read at address {address}"))
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
        // self.text[address as usize] = data;
        *self
            .text
            .get_mut(address as usize)
            .ok_or_eyre(eyre!("Invalid write at address {address}"))? = data;
        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        loop {
            let opcode = self.read_pc_inc()?;
            debug!("pc:{}, opcode:{:#x}", self.rf.pc, opcode);

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

                0xfa => {
                    trace!("LD A,(nn)");
                    let nn = self.consume_16bit_direct()?;
                    debug!("nn = {nn}");
                    // Not sure if this should be a read
                    let data = self.read(nn)?;
                    debug!("(nn) = {data}");
                    self.rf.a = data;
                }

                _ => todo!("Opcode not yet implemented: {opcode:#x}"),
            };
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

    fn pad_zeroes(bytes: &[u8]) -> [u8; 0xffff] {
        assert!(bytes.len() < 0xffff);
        let mut res = [0; 0xffff];
        res[..bytes.len()].copy_from_slice(&bytes);
        res
    }

    #[test]
    fn load_direct_16bit_immediate() {
        init();

        let mut cpu = SimpleDmg {
            rf: RegisterFile::default(),
            text: pad_zeroes(&[
                0x01, 0x34, 0x12, // LD BC,0x1234
                0x11, 0x78, 0x56, // LD DE,0x5678
                0x21, 0xad, 0xde, // LD HL,0xdead
                0x31, 0xef, 0xbe, // LD SP,0xbeef
                0x10, // STOP
            ]),
        };
        cpu.execute().unwrap();
        assert_eq!(cpu.rf.bc, 0x1234);
        assert_eq!(cpu.rf.de, 0x5678);
        assert_eq!(cpu.rf.hl, 0xdead);
        assert_eq!(cpu.rf.sp, 0xbeef);
        assert_eq!(cpu.rf.pc, 13);
    }

    #[test]
    fn load_accumulator_direct() {
        init();

        let mut cpu = SimpleDmg {
            rf: RegisterFile::default(),
            text: pad_zeroes(&[
                0xfa, 0x06, 0x00, // LD A,(0x0006) = load 0xbe into A
                0x10, // STOP
                0xde, 0xad, 0xbe, 0xef, // data
            ]),
        };
        cpu.execute();
        assert_eq!(cpu.rf.a, 0xbe);
        assert_eq!(cpu.rf.pc, 4);
    }

    #[test]
    fn load_indirect_8bit_a() {
        init();

        let mut cpu = SimpleDmg {
            rf: RegisterFile::default(),
            text: pad_zeroes(&[
                // Copy a single byte
                0xfa, 0x08, 0x00, // LD A,(0x0008) = load 0x88 into A
                0x01, 0x09, 0x00, // LD BC,0x09
                0x02, // LD (BC),A = write 0x88 to byte 9 (end)
                0x10, // STOP
                0x88, // source data
                0x00, // dest data
            ]),
        };
        cpu.execute();
        assert_eq!(cpu.rf.pc, 8);
        assert_eq!(cpu.text[8], cpu.text[9]);
    }
}
