#![allow(unused)]

use eyre::{OptionExt, Result, eyre};
use log::{debug, trace};

fn main() {
    todo!()
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
struct SimpleDmg<'a> {
    rf: RegisterFile,
    text: &'a mut [u8],
}

impl SimpleDmg<'_> {
    fn read(&self, address: u16) -> Result<u8> {
        self.text
            .get(address as usize)
            .copied()
            .ok_or_else(|| eyre!("Invalid read at address {address}"))
    }

    fn read_inc_pc(&mut self) -> Result<u8> {
        let instruction = self.read(self.rf.pc)?;
        self.rf.pc += 1;
        Ok(instruction)
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
            let opcode = self.read_inc_pc()?;
            debug!("pc:{}, opcode:{:#x}", self.rf.pc - 1, opcode);

            match opcode {
                // NOP
                0x00 => trace!("NOP"),
                // STOP
                0x10 => {
                    trace!("STOP");
                    break;
                }
                // LD rr,nn
                0x01 | 0x11 | 0x21 | 0x31 => {
                    let nn_lsb = self.read_inc_pc()?;
                    let nn_msb = self.read_inc_pc()?;
                    let nn = u16::from_le_bytes([nn_msb, nn_lsb]);
                    match opcode {
                        0x01 => {
                            self.rf.bc = nn;
                            trace!("LD BC, {nn}")
                        }
                        0x11 => {
                            self.rf.de = nn;
                            trace!("LD DE, {nn}")
                        }
                        0x21 => {
                            self.rf.hl = nn;
                            trace!("LD HL, {nn}")
                        }
                        0x31 => {
                            self.rf.sp = nn;
                            trace!("LD SP, {nn}")
                        }
                        _ => unreachable!(),
                    }
                }
                // LD (BC),A
                0x02 => self.write(self.rf.bc, self.rf.a)?,
                // LD (DE),A
                0x12 => self.write(self.rf.de, self.rf.a)?,
                // LD (HL+),A
                0x22 => {
                    self.write(self.rf.hl, self.rf.a)?;
                    self.rf.hl += 1
                }
                // LD (HL-),A
                0x32 => {
                    self.write(self.rf.hl, self.rf.a)?;
                    self.rf.hl -= 1
                }
                // INC BC
                0x03 => self.rf.bc += 1,
                // INC DE
                0x13 => self.rf.de += 1,
                // INC HL
                0x23 => self.rf.hl += 1,
                // INC SP
                0x33 => self.rf.sp += 1,

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

    #[test]
    fn inst_ld_rr_nn() {
        init();

        let mut text = [
            0x01, 0x12, 0x34, // LD BC, 0x1234
            0x11, 0x56, 0x78, // LD DE, 0x5678
            0x10, // STOP
        ];
        let mut cpu = SimpleDmg {
            rf: RegisterFile::default(),
            text: &mut text,
        };
        cpu.execute().unwrap();
        assert_eq!(cpu.rf.bc, 0x1234);
        assert_eq!(cpu.rf.de, 0x5678);
    }
}
