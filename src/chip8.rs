// CHIP-8 ROM parser and disassembler

/// CHIP-8 instruction
#[derive(Debug, Clone, Copy)]
pub struct Instruction {
    pub opcode: u16,
    pub addr: u16,  // Address in CHIP-8 memory (0x200 + offset)
}

impl Instruction {
    pub fn new(opcode: u16, addr: u16) -> Self {
        Self { opcode, addr }
    }

    /// Extract nibbles from opcode
    pub fn nibbles(&self) -> (u8, u8, u8, u8) {
        let n0 = ((self.opcode >> 12) & 0xF) as u8;
        let n1 = ((self.opcode >> 8) & 0xF) as u8;
        let n2 = ((self.opcode >> 4) & 0xF) as u8;
        let n3 = (self.opcode & 0xF) as u8;
        (n0, n1, n2, n3)
    }

    /// Get X register (second nibble)
    pub fn x(&self) -> u8 {
        ((self.opcode >> 8) & 0xF) as u8
    }

    /// Get Y register (third nibble)
    pub fn y(&self) -> u8 {
        ((self.opcode >> 4) & 0xF) as u8
    }

    /// Get N (last nibble)
    pub fn n(&self) -> u8 {
        (self.opcode & 0xF) as u8
    }

    /// Get NN (last byte)
    pub fn nn(&self) -> u8 {
        (self.opcode & 0xFF) as u8
    }

    /// Get NNN (last 12 bits - address)
    pub fn nnn(&self) -> u16 {
        self.opcode & 0xFFF
    }
}

/// Parse ROM into instructions
/// Stops parsing when an infinite loop (JP to self) is detected
pub fn parse(rom: &[u8]) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    let mut i = 0;

    while i + 1 < rom.len() {
        let opcode = ((rom[i] as u16) << 8) | (rom[i + 1] as u16);
        let addr = 0x200 + i as u16;
        instructions.push(Instruction::new(opcode, addr));

        // Check for infinite loop (JP to self)
        // This indicates end of code, rest is data
        let nibble0 = (opcode >> 12) & 0xF;
        if nibble0 == 0x1 {  // JP instruction
            let target = opcode & 0xFFF;
            if target == addr {
                // Infinite loop detected (JP to self), stop parsing
                break;
            }
        }

        i += 2;
    }

    instructions
}

/// Disassemble and print ROM
pub fn disassemble(rom: &[u8]) {
    let instructions = parse(rom);

    for inst in instructions {
        let mnemonic = disasm_instruction(&inst);
        println!("{:03X}: {:04X}  {}", inst.addr, inst.opcode, mnemonic);
    }
}

/// Disassemble a single instruction
pub fn disasm_instruction(inst: &Instruction) -> String {
    let (n0, n1, n2, n3) = inst.nibbles();

    match (n0, n1, n2, n3) {
        (0x0, 0x0, 0xE, 0x0) => "CLS".to_string(),
        (0x0, 0x0, 0xE, 0xE) => "RET".to_string(),
        (0x0, _, _, _) => format!("SYS  {:03X}", inst.nnn()),
        (0x1, _, _, _) => format!("JP   {:03X}", inst.nnn()),
        (0x2, _, _, _) => format!("CALL {:03X}", inst.nnn()),
        (0x3, _, _, _) => format!("SE   V{:X}, {:02X}", inst.x(), inst.nn()),
        (0x4, _, _, _) => format!("SNE  V{:X}, {:02X}", inst.x(), inst.nn()),
        (0x5, _, _, 0x0) => format!("SE   V{:X}, V{:X}", inst.x(), inst.y()),
        (0x6, _, _, _) => format!("LD   V{:X}, {:02X}", inst.x(), inst.nn()),
        (0x7, _, _, _) => format!("ADD  V{:X}, {:02X}", inst.x(), inst.nn()),
        (0x8, _, _, 0x0) => format!("LD   V{:X}, V{:X}", inst.x(), inst.y()),
        (0x8, _, _, 0x1) => format!("OR   V{:X}, V{:X}", inst.x(), inst.y()),
        (0x8, _, _, 0x2) => format!("AND  V{:X}, V{:X}", inst.x(), inst.y()),
        (0x8, _, _, 0x3) => format!("XOR  V{:X}, V{:X}", inst.x(), inst.y()),
        (0x8, _, _, 0x4) => format!("ADD  V{:X}, V{:X}", inst.x(), inst.y()),
        (0x8, _, _, 0x5) => format!("SUB  V{:X}, V{:X}", inst.x(), inst.y()),
        (0x8, _, _, 0x6) => format!("SHR  V{:X}", inst.x()),
        (0x8, _, _, 0x7) => format!("SUBN V{:X}, V{:X}", inst.x(), inst.y()),
        (0x8, _, _, 0xE) => format!("SHL  V{:X}", inst.x()),
        (0x9, _, _, 0x0) => format!("SNE  V{:X}, V{:X}", inst.x(), inst.y()),
        (0xA, _, _, _) => format!("LD   I, {:03X}", inst.nnn()),
        (0xB, _, _, _) => format!("JP   V0, {:03X}", inst.nnn()),
        (0xC, _, _, _) => format!("RND  V{:X}, {:02X}", inst.x(), inst.nn()),
        (0xD, _, _, _) => format!("DRW  V{:X}, V{:X}, {}", inst.x(), inst.y(), inst.n()),
        (0xE, _, 0x9, 0xE) => format!("SKP  V{:X}", inst.x()),
        (0xE, _, 0xA, 0x1) => format!("SKNP V{:X}", inst.x()),
        (0xF, _, 0x0, 0x7) => format!("LD   V{:X}, DT", inst.x()),
        (0xF, _, 0x0, 0xA) => format!("LD   V{:X}, K", inst.x()),
        (0xF, _, 0x1, 0x5) => format!("LD   DT, V{:X}", inst.x()),
        (0xF, _, 0x1, 0x8) => format!("LD   ST, V{:X}", inst.x()),
        (0xF, _, 0x1, 0xE) => format!("ADD  I, V{:X}", inst.x()),
        (0xF, _, 0x2, 0x9) => format!("LD   F, V{:X}", inst.x()),
        (0xF, _, 0x3, 0x3) => format!("LD   B, V{:X}", inst.x()),
        (0xF, _, 0x5, 0x5) => format!("LD   [I], V{:X}", inst.x()),
        (0xF, _, 0x6, 0x5) => format!("LD   V{:X}, [I]", inst.x()),
        _ => format!("??? {:04X}", inst.opcode),
    }
}
