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

    fn read_inc(&mut self, address: u16) -> Result<u8> {
        // Increment after reading so that if the read fails, we have a correct
        // PC for debugging.
        let res = self.read(address);
        self.rf.pc += 1;
        res
    }

    fn read_pc_inc(&mut self) -> Result<u8> {
        self.read_inc(self.rf.pc)
    }

    fn consume_16bit_direct(&mut self) -> Result<u16> {
        let nn_lsb = self.read_pc_inc()?;
        let nn_msb = self.read_pc_inc()?;
        Ok(u16::from_le_bytes([nn_msb, nn_lsb]))
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
                    self.rf.hl += 1
                }
                0x32 => {
                    trace!("LD (HL-), A");
                    self.write(self.rf.hl, self.rf.a)?;
                    self.rf.hl -= 1
                }

                0x03 => {
                    trace!("INC BC");
                    self.rf.bc += 1
                }
                0x13 => {
                    trace!("INC DE");
                    self.rf.de += 1
                }
                0x23 => {
                    trace!("INC HL");
                    self.rf.hl += 1
                }
                0x33 => {
                    trace!("INC SP");
                    self.rf.sp += 1
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

    #[test]
    fn load_direct_16bit_immediate() {
        init();

        let mut text = [
            0x01, 0x12, 0x34, // LD BC,0x1234
            0x11, 0x56, 0x78, // LD DE,0x5678
            0x21, 0xde, 0xad, // LD HL,0xdead
            0x31, 0xbe, 0xef, // LD SP,0xbeef
            0x10, // STOP
        ];
        let mut cpu = SimpleDmg {
            rf: RegisterFile::default(),
            text: &mut text,
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

        let mut text = [
            0xfa, 0x00, 0x06, // LD A,(0x0006) = load 0xbe into A
            0x10, // STOP
            0xde, 0xad, 0xbe, 0xef, // data
        ];

        let mut cpu = SimpleDmg {
            rf: RegisterFile::default(),
            text: &mut text,
        };
        cpu.execute();
        assert_eq!(cpu.rf.a, 0xbe);
        assert_eq!(cpu.rf.pc, 4);
    }

    #[test]
    fn load_indirect_8bit_a() {
        init();

        // Copy a single byte
        let mut text = [
            0xfa, 0x00, 0x08, // LD A,(0x0008) = load 0x88 into A
            0x01, 0x00, 0x09, // LD BC,0x09
            0x02, // LD (BC),A = write 0x88 to byte 9 (end)
            0x10, // STOP
            0x88, // source data
            0x00, // dest data
        ];
        let mut cpu = SimpleDmg {
            rf: RegisterFile::default(),
            text: &mut text,
        };
        cpu.execute();
        assert_eq!(cpu.rf.pc, 8);
        assert_eq!(text[8], text[9]);
    }
}
