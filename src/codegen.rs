// Z80 Code Generator for CHIP-8
// Compiles CHIP-8 instructions to native Z80 code

use crate::chip8::{self, Instruction};
use std::collections::HashMap;

/// Memory layout for RetroShield Z80 (32KB ROM)
/// 0x0000-0x00FF: RST vectors
/// 0x0100-0x7FFF: Z80 native code (compiled CHIP-8 + runtime) - 32KB ROM
/// 0x8000-0x80FF: CHIP-8 registers (V0-VF, I, PC, SP, DT, ST)
/// 0x8100-0x81FF: CHIP-8 stack (16 levels x 2 bytes)
/// 0x8200-0x82FF: Display buffer (64x32 = 256 bytes)
/// 0x8300-0x83FF: Font data (16 chars x 5 bytes = 80 bytes)
/// 0x8400-0xFFFF: CHIP-8 RAM (for data, not code)

const CODE_START: u16 = 0x0100;
// RAM must be at >= 0x8000 (above 32KB ROM area) for emulator compatibility
const CHIP8_V0: u16 = 0x8000;      // V0-VF registers (16 bytes)
const CHIP8_I: u16 = 0x8010;       // I register (2 bytes)
const CHIP8_SP: u16 = 0x8012;      // Stack pointer (1 byte)
const CHIP8_DT: u16 = 0x8013;      // Delay timer (1 byte)
const CHIP8_ST: u16 = 0x8014;      // Sound timer (1 byte)
const CHIP8_KEY: u16 = 0x8015;     // Current key pressed (1 byte, 0xFF = none)
const CHIP8_RNG: u16 = 0x8016;     // RNG state (2 bytes)
const CHIP8_STACK: u16 = 0x8100;   // Call stack (32 bytes)
const DISPLAY_BUF: u16 = 0x8200;   // 64x32 / 8 = 256 bytes
const FONT_DATA: u16 = 0x8300;     // Sprite font
const CHIP8_RAM: u16 = 0x8400;     // General RAM

// ACIA ports
const ACIA_CTRL: u8 = 0x80;
const ACIA_DATA: u8 = 0x81;

pub struct Compiler {
    code: Vec<u8>,
    pc: u16,
    labels: HashMap<String, u16>,
    forward_refs: Vec<(u16, String)>,
    chip8_labels: HashMap<u16, String>,  // CHIP-8 addr -> Z80 label
    chip8_rom: Vec<u8>,                  // Original CHIP-8 ROM data
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            pc: 0,  // Start at 0, not CODE_START
            labels: HashMap::new(),
            forward_refs: Vec::new(),
            chip8_labels: HashMap::new(),
            chip8_rom: Vec::new(),
        }
    }

    pub fn compile(&mut self, rom: &[u8]) -> Result<Vec<u8>, String> {
        // Store original ROM for sprite data access
        self.chip8_rom = rom.to_vec();

        // Parse CHIP-8 instructions
        let instructions = chip8::parse(rom);

        // First pass: create labels for all CHIP-8 addresses
        for inst in &instructions {
            let label = format!("c8_{:03X}", inst.addr);
            self.chip8_labels.insert(inst.addr, label);
        }

        // Generate Z80 code
        self.generate_header();
        self.generate_init();
        self.generate_runtime();

        // Main entry point - jump to first CHIP-8 instruction
        self.label("main");
        if !instructions.is_empty() {
            let first_label = format!("c8_{:03X}", 0x200);
            self.jp_label(&first_label);
        } else {
            self.jp_label("halt");
        }

        // Compile each CHIP-8 instruction
        for inst in &instructions {
            let label = format!("c8_{:03X}", inst.addr);
            self.label(&label);
            self.compile_instruction(inst)?;
        }

        // Generate halt
        self.label("halt");
        self.emit(0x76);  // HALT
        self.jp_label("halt");

        // Embed CHIP-8 ROM data for custom sprite access
        // This label marks the start of embedded ROM (corresponds to CHIP-8 address 0x200)
        self.label("chip8_rom_data");
        for byte in &self.chip8_rom.clone() {
            self.emit(*byte);
        }

        // Resolve forward references
        self.resolve_refs()?;

        // Create 32KB ROM image
        let mut rom_image = vec![0u8; 32768];

        // Copy code
        for (i, byte) in self.code.iter().enumerate() {
            if i < rom_image.len() {
                rom_image[i] = *byte;
            }
        }

        // Embed font data at FONT_DATA (but in ROM, we mirror at code location)
        self.embed_font(&mut rom_image);

        Ok(rom_image)
    }

    fn generate_header(&mut self) {
        // RST 0 - entry point
        self.emit(0xC3);  // JP
        self.emit16(CODE_START);

        // Pad to CODE_START
        while self.pc < CODE_START {
            self.emit(0x00);
        }
    }

    fn generate_init(&mut self) {
        self.label("init");

        // Initialize stack pointer (at top of RAM, grows downward)
        self.emit(0x31);  // LD SP, nn
        self.emit16(0x0000);  // SP = 0x10000 wraps to 0x0000, grows down into 0xFFFF

        // Initialize ACIA
        self.call_label("acia_init");

        // Clear CHIP-8 registers
        self.ld_hl_nn(CHIP8_V0);
        self.ld_bc_nn(32);  // Clear V0-VF + I + misc
        self.label("init_clear");
        self.xor_a();       // A = 0 (must be inside loop!)
        self.ld_hl_a();
        self.inc_hl();
        self.dec_bc();
        self.ld_a_b();
        self.or_c();
        self.jr_nz("init_clear");

        // Initialize RNG seed
        self.ld_hl_nn(CHIP8_RNG);
        self.ld_a_n(0xAC);
        self.ld_hl_a();
        self.inc_hl();
        self.ld_a_n(0xE1);
        self.ld_hl_a();

        // Clear display
        self.call_label("cls");

        // Copy font to RAM
        self.call_label("copy_font");

        // Print banner
        self.call_label("print_banner");

        // Jump to main
        self.jp_label("main");
    }

    fn generate_runtime(&mut self) {
        // ACIA init
        self.label("acia_init");
        self.ld_a_n(0x03);  // Master reset
        self.out_n_a(ACIA_CTRL);
        self.ld_a_n(0x15);  // 8N1, /16
        self.out_n_a(ACIA_CTRL);
        self.ret();

        // Print character in A
        self.label("print_char");
        self.push_af();
        self.label("print_wait");
        self.in_a_n(ACIA_CTRL);
        self.emit(0xE6); self.emit(0x02);  // AND 2
        self.jr_z("print_wait");
        self.pop_af();
        self.out_n_a(ACIA_DATA);
        self.ret();

        // Print banner
        self.label("print_banner");
        self.ld_hl_label("banner_str");
        self.label("print_str_loop");
        self.ld_a_hl();
        self.or_a();
        self.ret_z();
        self.call_label("print_char");
        self.inc_hl();
        self.jr_label("print_str_loop");

        // Banner string
        self.label("banner_str");
        for b in b"CHIP-8 on Z80\r\n" {
            self.emit(*b);
        }
        self.emit(0);

        // CLS - Clear screen
        self.label("cls");
        self.ld_hl_nn(DISPLAY_BUF);
        self.ld_bc_nn(256);
        self.label("cls_loop");
        self.xor_a();       // A = 0 (must be inside loop!)
        self.ld_hl_a();
        self.inc_hl();
        self.dec_bc();
        self.ld_a_b();
        self.or_c();
        self.jr_nz("cls_loop");
        // Refresh display to show cleared screen
        self.jp_label("refresh_display");

        // Copy font data
        self.label("copy_font");
        self.ld_hl_label("font_rom");
        self.ld_de_nn(FONT_DATA);
        self.ld_bc_nn(80);  // 16 chars x 5 bytes
        self.label("copy_font_loop");
        self.ld_a_hl();
        self.ld_de_a();
        self.inc_hl();
        self.inc_de();
        self.dec_bc();
        self.ld_a_b();
        self.or_c();
        self.jr_nz("copy_font_loop");
        self.ret();

        // Font ROM data (0-F sprites, 5 bytes each)
        self.label("font_rom");
        // 0
        self.emit(0xF0); self.emit(0x90); self.emit(0x90); self.emit(0x90); self.emit(0xF0);
        // 1
        self.emit(0x20); self.emit(0x60); self.emit(0x20); self.emit(0x20); self.emit(0x70);
        // 2
        self.emit(0xF0); self.emit(0x10); self.emit(0xF0); self.emit(0x80); self.emit(0xF0);
        // 3
        self.emit(0xF0); self.emit(0x10); self.emit(0xF0); self.emit(0x10); self.emit(0xF0);
        // 4
        self.emit(0x90); self.emit(0x90); self.emit(0xF0); self.emit(0x10); self.emit(0x10);
        // 5
        self.emit(0xF0); self.emit(0x80); self.emit(0xF0); self.emit(0x10); self.emit(0xF0);
        // 6
        self.emit(0xF0); self.emit(0x80); self.emit(0xF0); self.emit(0x90); self.emit(0xF0);
        // 7
        self.emit(0xF0); self.emit(0x10); self.emit(0x20); self.emit(0x40); self.emit(0x40);
        // 8
        self.emit(0xF0); self.emit(0x90); self.emit(0xF0); self.emit(0x90); self.emit(0xF0);
        // 9
        self.emit(0xF0); self.emit(0x90); self.emit(0xF0); self.emit(0x10); self.emit(0xF0);
        // A
        self.emit(0xF0); self.emit(0x90); self.emit(0xF0); self.emit(0x90); self.emit(0x90);
        // B
        self.emit(0xE0); self.emit(0x90); self.emit(0xE0); self.emit(0x90); self.emit(0xE0);
        // C
        self.emit(0xF0); self.emit(0x80); self.emit(0x80); self.emit(0x80); self.emit(0xF0);
        // D
        self.emit(0xE0); self.emit(0x90); self.emit(0x90); self.emit(0x90); self.emit(0xE0);
        // E
        self.emit(0xF0); self.emit(0x80); self.emit(0xF0); self.emit(0x80); self.emit(0xF0);
        // F
        self.emit(0xF0); self.emit(0x80); self.emit(0xF0); self.emit(0x80); self.emit(0x80);

        // RNG - Simple LFSR
        self.label("rng");
        self.ld_hl_nn(CHIP8_RNG);
        self.ld_a_hl();
        self.inc_hl();
        self.ld_h_hl();
        self.ld_l_a();
        // LFSR: x ^= x << 7; x ^= x >> 9; x ^= x << 8
        self.add_hl_hl();  // Simplified: just rotate
        self.emit(0xCB); self.emit(0x15);  // RL L
        self.emit(0xCB); self.emit(0x14);  // RL H
        self.ld_a_l();
        self.xor_h();
        self.ld_l_a();
        // Store back
        self.push_hl();
        self.ld_hl_nn(CHIP8_RNG);
        self.pop_de();
        self.ld_a_e();
        self.ld_hl_a();
        self.inc_hl();
        self.ld_a_d();
        self.ld_hl_a();
        self.ld_a_e();  // Return random byte in A
        self.ret();

        // Get key - check for serial input
        self.label("get_key");
        self.in_a_n(ACIA_CTRL);
        self.emit(0xE6); self.emit(0x01);  // AND 1
        self.ret_z();  // No key, A=0
        self.in_a_n(ACIA_DATA);
        // Map ASCII to CHIP-8 keys (0-9, A-F)
        self.cp_n(b'0');
        self.jr_c("get_key_alpha");
        self.cp_n(b'9' + 1);
        self.jr_nc("get_key_alpha");
        self.sub_n(b'0');  // 0-9
        self.ret();
        self.label("get_key_alpha");
        self.cp_n(b'a');
        self.jr_c("get_key_upper");
        self.cp_n(b'f' + 1);
        self.jr_nc("get_key_none");
        self.sub_n(b'a' - 10);  // a-f -> 10-15
        self.ret();
        self.label("get_key_upper");
        self.cp_n(b'A');
        self.jr_c("get_key_none");
        self.cp_n(b'F' + 1);
        self.jr_nc("get_key_none");
        self.sub_n(b'A' - 10);  // A-F -> 10-15
        self.ret();
        self.label("get_key_none");
        self.ld_a_n(0xFF);
        self.ret();

        // Wait for key - blocking
        self.label("wait_key");
        self.call_label("get_key");
        self.cp_n(0xFF);
        self.jr_z("wait_key");
        self.ret();

        // Draw sprite: DE = screen addr, HL = sprite addr, B = height
        // Returns VF in A (1 if collision)
        self.label("draw_sprite");
        self.xor_a();
        self.ld_c_a();  // C = collision flag
        self.label("draw_row");
        // Get sprite byte
        self.ld_a_hl();  // A = sprite byte
        self.push_hl();  // Save sprite pointer
        self.push_de();  // Save screen pointer
        // XOR with screen
        self.ex_de_hl();   // HL = screen addr
        self.ld_e_a();     // E = sprite byte
        self.ld_a_hl();    // A = screen byte
        self.push_af();    // Save screen byte
        self.ld_a_e();     // A = sprite byte
        self.xor_hl();     // A = sprite XOR screen
        self.ld_hl_a();    // Write XOR result to screen
        self.pop_af();     // A = original screen byte
        self.and_a_e();    // A = screen AND sprite (pixels that collided)
        self.or_c();
        self.ld_c_a();     // Update collision flag
        // Restore and advance pointers
        self.pop_de();     // DE = screen addr
        self.pop_hl();     // HL = sprite addr
        self.inc_hl();     // Next sprite byte
        // Screen += 8 (next row)
        self.push_hl();
        self.ld_hl_nn(8);
        self.add_hl_de();
        self.ex_de_hl();   // DE = screen + 8
        self.pop_hl();     // HL = sprite
        self.dec_b();
        self.jr_nz("draw_row");
        self.ld_a_c();
        self.or_a();
        self.ret_z();
        self.ld_a_n(1);
        self.ret();

        // Refresh display to terminal (ANSI)
        self.label("refresh_display");
        // Move cursor to row 2 (below banner) - ESC[2;1H
        self.ld_a_n(0x1B);
        self.call_label("print_char");
        self.ld_a_n(b'[');
        self.call_label("print_char");
        self.ld_a_n(b'2');
        self.call_label("print_char");
        self.ld_a_n(b';');
        self.call_label("print_char");
        self.ld_a_n(b'1');
        self.call_label("print_char");
        self.ld_a_n(b'H');
        self.call_label("print_char");

        self.ld_hl_nn(DISPLAY_BUF);
        self.ld_d_n(32);  // 32 rows
        self.label("refresh_row");
        self.ld_e_n(8);   // 8 bytes per row (64 pixels)
        self.label("refresh_byte");
        self.ld_a_hl();
        self.ld_b_n(8);   // 8 bits per byte
        self.label("refresh_bit");
        self.emit(0xCB); self.emit(0x07);  // RLC A - rotate left
        self.push_af();
        self.jr_nc("refresh_space");
        self.ld_a_n(b'#');
        self.jr_label("refresh_out");
        self.label("refresh_space");
        self.ld_a_n(b' ');
        self.label("refresh_out");
        self.call_label("print_char");
        self.pop_af();
        self.dec_b();
        self.jr_nz("refresh_bit");
        self.inc_hl();
        self.dec_e();
        self.jr_nz("refresh_byte");
        // Newline
        self.ld_a_n(b'\r');
        self.call_label("print_char");
        self.ld_a_n(b'\n');
        self.call_label("print_char");
        self.dec_d();
        self.jr_nz("refresh_row");
        self.ret();
    }

    fn compile_instruction(&mut self, inst: &Instruction) -> Result<(), String> {
        let (n0, n1, n2, n3) = inst.nibbles();

        match (n0, n1, n2, n3) {
            // 00E0 - CLS
            (0x0, 0x0, 0xE, 0x0) => {
                self.call_label("cls");
            }

            // 00EE - RET
            (0x0, 0x0, 0xE, 0xE) => {
                // Pop return address from CHIP-8 stack
                self.ld_hl_nn(CHIP8_SP);
                self.dec_hl();
                self.ld_a_hl();  // SP
                self.dec_a();
                self.ld_hl_a();  // SP--
                // Get address from stack
                self.ld_l_a();
                self.ld_h_n(0);
                self.add_hl_hl();  // *2
                self.ld_de_nn(CHIP8_STACK);
                self.add_hl_de();
                self.ld_e_hl();
                self.inc_hl();
                self.ld_d_hl();
                // Jump to DE
                self.push_de();
                self.ret();  // RET pops address
            }

            // 0NNN - SYS (ignored on modern interpreters)
            (0x0, _, _, _) => {
                // NOP
            }

            // 1NNN - JP addr
            (0x1, _, _, _) => {
                let addr = inst.nnn();
                if let Some(label) = self.chip8_labels.get(&addr) {
                    self.jp_label(&label.clone());
                } else {
                    return Err(format!("Jump to unknown address {:03X}", addr));
                }
            }

            // 2NNN - CALL addr
            (0x2, _, _, _) => {
                let addr = inst.nnn();
                // Push return address to CHIP-8 stack
                // Return address is next CHIP-8 instruction
                let ret_addr = inst.addr + 2;
                self.ld_hl_nn(CHIP8_SP);
                self.ld_a_hl();  // A = SP
                self.ld_l_a();
                self.ld_h_n(0);
                self.add_hl_hl();  // *2
                self.ld_de_nn(CHIP8_STACK);
                self.add_hl_de();
                // Store return address
                self.ld_a_n((ret_addr & 0xFF) as u8);
                self.ld_hl_a();
                self.inc_hl();
                self.ld_a_n((ret_addr >> 8) as u8);
                self.ld_hl_a();
                // Increment SP
                self.ld_hl_nn(CHIP8_SP);
                self.inc_hl_ind();
                // Jump to subroutine
                if let Some(label) = self.chip8_labels.get(&addr) {
                    self.jp_label(&label.clone());
                } else {
                    return Err(format!("Call to unknown address {:03X}", addr));
                }
            }

            // 3XNN - SE Vx, byte (skip if equal)
            (0x3, _, _, _) => {
                let x = inst.x();
                let nn = inst.nn();
                // Load Vx
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.cp_n(nn);
                // Skip next instruction if equal
                let next_addr = inst.addr + 4;  // Skip 2 bytes (one CHIP-8 instruction)
                if let Some(label) = self.chip8_labels.get(&next_addr) {
                    self.jp_z_label(&label.clone());
                } else {
                    eprintln!("Warning: SE at {:03X} skip target {:03X} has no label", inst.addr, next_addr);
                }
            }

            // 4XNN - SNE Vx, byte (skip if not equal)
            (0x4, _, _, _) => {
                let x = inst.x();
                let nn = inst.nn();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.cp_n(nn);
                let next_addr = inst.addr + 4;
                if let Some(label) = self.chip8_labels.get(&next_addr) {
                    self.jp_nz_label(&label.clone());
                }
            }

            // 5XY0 - SE Vx, Vy
            (0x5, _, _, 0x0) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_hl_nn(CHIP8_V0 + y as u16);
                self.cp_hl();
                let next_addr = inst.addr + 4;
                if let Some(label) = self.chip8_labels.get(&next_addr) {
                    self.jp_z_label(&label.clone());
                }
            }

            // 6XNN - LD Vx, byte
            (0x6, _, _, _) => {
                let x = inst.x();
                let nn = inst.nn();
                self.ld_a_n(nn);
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // 7XNN - ADD Vx, byte
            (0x7, _, _, _) => {
                let x = inst.x();
                let nn = inst.nn();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.add_a_n(nn);
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // 8XY0 - LD Vx, Vy
            (0x8, _, _, 0x0) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + y as u16);
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // 8XY1 - OR Vx, Vy
            (0x8, _, _, 0x1) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_hl_nn(CHIP8_V0 + y as u16);
                self.or_hl();
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // 8XY2 - AND Vx, Vy
            (0x8, _, _, 0x2) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_hl_nn(CHIP8_V0 + y as u16);
                self.and_hl();
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // 8XY3 - XOR Vx, Vy
            (0x8, _, _, 0x3) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_hl_nn(CHIP8_V0 + y as u16);
                self.xor_hl();
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // 8XY4 - ADD Vx, Vy (with carry to VF)
            (0x8, _, _, 0x4) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_hl_nn(CHIP8_V0 + y as u16);
                self.add_a_hl();
                self.ld_mem_a(CHIP8_V0 + x as u16);
                // Set VF to carry
                self.ld_a_n(0);
                self.emit(0xCE); self.emit(0x00);  // ADC A, 0
                self.ld_mem_a(CHIP8_V0 + 0xF);
            }

            // 8XY5 - SUB Vx, Vy (VF = NOT borrow)
            (0x8, _, _, 0x5) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_hl_nn(CHIP8_V0 + y as u16);
                self.sub_hl();
                self.ld_mem_a(CHIP8_V0 + x as u16);
                // VF = NOT borrow (1 if no borrow)
                self.ld_a_n(1);
                self.jr_nc("no_borrow_8xy5");
                self.xor_a();
                self.label("no_borrow_8xy5");
                self.ld_mem_a(CHIP8_V0 + 0xF);
            }

            // 8XY6 - SHR Vx (VF = LSB)
            (0x8, _, _, 0x6) => {
                let x = inst.x();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.emit(0xCB); self.emit(0x3F);  // SRL A
                self.ld_mem_a(CHIP8_V0 + x as u16);
                // VF = old LSB
                self.ld_a_n(0);
                self.emit(0xCE); self.emit(0x00);  // ADC A, 0
                self.ld_mem_a(CHIP8_V0 + 0xF);
            }

            // 8XY7 - SUBN Vx, Vy (Vx = Vy - Vx, VF = NOT borrow)
            (0x8, _, _, 0x7) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + y as u16);
                self.ld_hl_nn(CHIP8_V0 + x as u16);
                self.sub_hl();
                self.ld_mem_a(CHIP8_V0 + x as u16);
                self.ld_a_n(1);
                self.jr_nc("no_borrow_8xy7");
                self.xor_a();
                self.label("no_borrow_8xy7");
                self.ld_mem_a(CHIP8_V0 + 0xF);
            }

            // 8XYE - SHL Vx (VF = MSB)
            (0x8, _, _, 0xE) => {
                let x = inst.x();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.emit(0xCB); self.emit(0x27);  // SLA A
                self.ld_mem_a(CHIP8_V0 + x as u16);
                // VF = old MSB (now in carry)
                self.ld_a_n(0);
                self.emit(0xCE); self.emit(0x00);  // ADC A, 0
                self.ld_mem_a(CHIP8_V0 + 0xF);
            }

            // 9XY0 - SNE Vx, Vy
            (0x9, _, _, 0x0) => {
                let x = inst.x();
                let y = inst.y();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_hl_nn(CHIP8_V0 + y as u16);
                self.cp_hl();
                let next_addr = inst.addr + 4;
                if let Some(label) = self.chip8_labels.get(&next_addr) {
                    self.jp_nz_label(&label.clone());
                }
            }

            // ANNN - LD I, addr
            (0xA, _, _, _) => {
                let nnn = inst.nnn();
                self.ld_hl_nn(nnn);
                self.ld_de_nn(CHIP8_I);
                self.ld_a_l();
                self.ld_de_a();
                self.inc_de();
                self.ld_a_h();
                self.ld_de_a();
            }

            // BNNN - JP V0, addr
            (0xB, _, _, _) => {
                let nnn = inst.nnn();
                self.ld_a_mem(CHIP8_V0);
                self.ld_l_a();
                self.ld_h_n(0);
                self.ld_de_nn(nnn);
                self.add_hl_de();
                // This is tricky for static compilation - need runtime jump table
                // For now, just use a simple computed jump
                self.push_hl();
                self.ret();  // Jump to HL
            }

            // CXNN - RND Vx, byte
            (0xC, _, _, _) => {
                let x = inst.x();
                let nn = inst.nn();
                self.call_label("rng");
                self.and_n(nn);
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // DXYN - DRW Vx, Vy, nibble
            (0xD, _, _, _) => {
                let x = inst.x();
                let y = inst.y();
                let n = inst.n();

                // Calculate screen address: (Vy * 8) + (Vx / 8) + DISPLAY_BUF
                // For simplicity, we'll use byte-aligned X
                self.ld_a_mem(CHIP8_V0 + y as u16);
                self.emit(0xE6); self.emit(0x1F);  // AND 31 (wrap Y)
                self.ld_l_a();
                self.ld_h_n(0);
                // *8 (8 bytes per row)
                self.add_hl_hl();
                self.add_hl_hl();
                self.add_hl_hl();
                // Add X/8
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.emit(0xE6); self.emit(0x3F);  // AND 63 (wrap X)
                self.emit(0xCB); self.emit(0x3F);  // SRL A (divide by 2)
                self.emit(0xCB); self.emit(0x3F);  // SRL A (divide by 4)
                self.emit(0xCB); self.emit(0x3F);  // SRL A (divide by 8)
                self.ld_e_a();
                self.ld_d_n(0);
                self.add_hl_de();
                self.ld_de_nn(DISPLAY_BUF);
                self.add_hl_de();
                self.push_hl();  // Save screen address

                // Get sprite address from I
                self.ld_hl_nn(CHIP8_I);
                self.ld_e_hl();
                self.inc_hl();
                self.ld_d_hl();
                // Add FONT_DATA base if I < 0x50 (font sprite)
                // Use unique labels per DRW to avoid conflicts
                let not_font_label = format!("draw_not_font_{:03X}", inst.addr);
                let have_sprite_label = format!("draw_have_sprite_{:03X}", inst.addr);
                self.ld_a_d();
                self.or_a();
                self.jr_nz(&not_font_label);
                self.ld_a_e();
                self.cp_n(0x50);  // Font data is 0-0x50
                self.jr_nc(&not_font_label);
                // Font sprite: HL = FONT_DATA + I
                self.ld_hl_nn(FONT_DATA);
                self.add_hl_de();
                self.jr_label(&have_sprite_label);
                self.label(&not_font_label);
                // Custom sprite: I is CHIP-8 address (>= 0x200)
                // Convert to Z80 address: chip8_rom_data + (I - 0x200)
                // Since chip8_rom_data corresponds to CHIP-8 0x200, we just add the offset
                self.ld_hl_nn(0x200);  // Subtract CHIP-8 base
                self.ex_de_hl();       // DE = 0x200, HL = I
                self.or_a();           // Clear carry
                self.sbc_hl_de();      // HL = I - 0x200
                self.ex_de_hl();       // DE = I - 0x200
                self.ld_hl_label("chip8_rom_data");
                self.add_hl_de();      // HL = chip8_rom_data + (I - 0x200)
                self.label(&have_sprite_label);
                // HL = sprite address
                self.pop_de();  // DE = screen address
                self.ld_b_n(n);
                self.call_label("draw_sprite");
                // Store VF
                self.ld_mem_a(CHIP8_V0 + 0xF);
                // Refresh display
                self.call_label("refresh_display");
            }

            // EX9E - SKP Vx (skip if key pressed)
            (0xE, _, 0x9, 0xE) => {
                let x = inst.x();
                self.call_label("get_key");
                self.ld_hl_nn(CHIP8_V0 + x as u16);
                self.cp_hl();
                let next_addr = inst.addr + 4;
                if let Some(label) = self.chip8_labels.get(&next_addr) {
                    self.jp_z_label(&label.clone());
                }
            }

            // EXA1 - SKNP Vx (skip if key not pressed)
            (0xE, _, 0xA, 0x1) => {
                let x = inst.x();
                self.call_label("get_key");
                self.ld_hl_nn(CHIP8_V0 + x as u16);
                self.cp_hl();
                let next_addr = inst.addr + 4;
                if let Some(label) = self.chip8_labels.get(&next_addr) {
                    self.jp_nz_label(&label.clone());
                }
            }

            // FX07 - LD Vx, DT
            (0xF, _, 0x0, 0x7) => {
                let x = inst.x();
                self.ld_a_mem(CHIP8_DT);
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // FX0A - LD Vx, K (wait for key)
            (0xF, _, 0x0, 0xA) => {
                let x = inst.x();
                self.call_label("wait_key");
                self.ld_mem_a(CHIP8_V0 + x as u16);
            }

            // FX15 - LD DT, Vx
            (0xF, _, 0x1, 0x5) => {
                let x = inst.x();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_mem_a(CHIP8_DT);
            }

            // FX18 - LD ST, Vx
            (0xF, _, 0x1, 0x8) => {
                let x = inst.x();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_mem_a(CHIP8_ST);
            }

            // FX1E - ADD I, Vx
            (0xF, _, 0x1, 0xE) => {
                let x = inst.x();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.ld_l_a();
                self.ld_h_n(0);
                self.ld_de_nn(CHIP8_I);
                self.push_de();
                self.ld_a_de();
                self.ld_e_a();
                self.inc_de();
                self.ld_a_de();
                self.ld_d_a();
                self.add_hl_de();
                self.pop_de();
                self.ld_a_l();
                self.ld_de_a();
                self.inc_de();
                self.ld_a_h();
                self.ld_de_a();
            }

            // FX29 - LD F, Vx (point I to font sprite)
            (0xF, _, 0x2, 0x9) => {
                let x = inst.x();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                self.emit(0xE6); self.emit(0x0F);  // AND 0x0F
                // Multiply by 5 (each font char is 5 bytes)
                self.ld_l_a();
                self.ld_h_n(0);
                self.add_hl_hl();  // *2
                self.add_hl_hl();  // *4
                self.ld_e_a();
                self.ld_d_n(0);
                self.add_hl_de();  // *5 = offset into font (0-0x4F)
                // Store offset in I (don't add FONT_DATA here - DRW will handle it)
                self.ld_de_nn(CHIP8_I);
                self.ld_a_l();
                self.ld_de_a();
                self.inc_de();
                self.ld_a_h();
                self.ld_de_a();
            }

            // FX33 - LD B, Vx (BCD)
            (0xF, _, 0x3, 0x3) => {
                let x = inst.x();
                self.ld_a_mem(CHIP8_V0 + x as u16);
                // Get I address
                self.ld_hl_nn(CHIP8_I);
                self.ld_e_hl();
                self.inc_hl();
                self.ld_d_hl();
                // Add RAM base
                self.ld_hl_nn(CHIP8_RAM - 0x200);
                self.add_hl_de();
                // Store hundreds
                self.ld_b_n(0);
                self.label("bcd_hundreds");
                self.cp_n(100);
                self.jr_c("bcd_tens");
                self.sub_n(100);
                self.inc_b();
                self.jr_label("bcd_hundreds");
                self.label("bcd_tens");
                self.push_af();
                self.ld_a_b();
                self.ld_hl_a();
                self.inc_hl();
                self.pop_af();
                // Store tens
                self.ld_b_n(0);
                self.label("bcd_tens_loop");
                self.cp_n(10);
                self.jr_c("bcd_ones");
                self.sub_n(10);
                self.inc_b();
                self.jr_label("bcd_tens_loop");
                self.label("bcd_ones");
                self.push_af();
                self.ld_a_b();
                self.ld_hl_a();
                self.inc_hl();
                self.pop_af();
                // Store ones
                self.ld_hl_a();
            }

            // FX55 - LD [I], Vx (store V0-Vx)
            (0xF, _, 0x5, 0x5) => {
                let x = inst.x();
                // Get I
                self.ld_hl_nn(CHIP8_I);
                self.ld_e_hl();
                self.inc_hl();
                self.ld_d_hl();
                self.ld_hl_nn(CHIP8_RAM - 0x200);
                self.add_hl_de();
                self.ex_de_hl();  // DE = destination
                self.ld_hl_nn(CHIP8_V0);
                self.ld_b_n(x + 1);
                self.label("store_regs");
                self.ld_a_hl();
                self.ld_de_a();
                self.inc_hl();
                self.inc_de();
                self.dec_b();
                self.jr_nz("store_regs");
            }

            // FX65 - LD Vx, [I] (load V0-Vx)
            (0xF, _, 0x6, 0x5) => {
                let x = inst.x();
                // Get I
                self.ld_hl_nn(CHIP8_I);
                self.ld_e_hl();
                self.inc_hl();
                self.ld_d_hl();
                self.ld_hl_nn(CHIP8_RAM - 0x200);
                self.add_hl_de();  // HL = source
                self.ld_de_nn(CHIP8_V0);
                self.ld_b_n(x + 1);
                self.label("load_regs");
                self.ld_a_hl();
                self.ld_de_a();
                self.inc_hl();
                self.inc_de();
                self.dec_b();
                self.jr_nz("load_regs");
            }

            _ => {
                // Unknown opcode - NOP
            }
        }

        Ok(())
    }

    fn embed_font(&self, _rom: &mut [u8]) {
        // Font is already embedded in code via font_rom label
    }

    // Helper methods for emitting Z80 code
    fn emit(&mut self, byte: u8) {
        self.code.push(byte);
        self.pc += 1;
    }

    fn emit16(&mut self, word: u16) {
        self.emit((word & 0xFF) as u8);
        self.emit((word >> 8) as u8);
    }

    fn label(&mut self, name: &str) {
        self.labels.insert(name.to_string(), self.pc);
    }

    fn emit_label_ref(&mut self, name: &str) {
        self.forward_refs.push((self.pc, name.to_string()));
        self.emit16(0);  // Placeholder
    }

    fn resolve_refs(&mut self) -> Result<(), String> {
        for (addr, name) in &self.forward_refs {
            let target = self.labels.get(name)
                .ok_or_else(|| format!("Undefined label: {}", name))?;
            let offset = *addr as usize;  // Direct index since pc starts at 0
            self.code[offset] = (*target & 0xFF) as u8;
            self.code[offset + 1] = (*target >> 8) as u8;
        }
        Ok(())
    }

    // Z80 instruction helpers
    fn jp_label(&mut self, label: &str) {
        self.emit(0xC3);
        self.emit_label_ref(label);
    }

    fn jp_z_label(&mut self, label: &str) {
        self.emit(0xCA);
        self.emit_label_ref(label);
    }

    fn jp_nz_label(&mut self, label: &str) {
        self.emit(0xC2);
        self.emit_label_ref(label);
    }

    fn jr_label(&mut self, label: &str) {
        // For simplicity, use JP instead of JR for labels
        self.jp_label(label);
    }

    fn jr_z(&mut self, label: &str) {
        self.jp_z_label(label);
    }

    fn jr_nz(&mut self, label: &str) {
        self.jp_nz_label(label);
    }

    fn jr_c(&mut self, label: &str) {
        self.emit(0xDA);  // JP C
        self.emit_label_ref(label);
    }

    fn jr_nc(&mut self, label: &str) {
        self.emit(0xD2);  // JP NC
        self.emit_label_ref(label);
    }

    fn call_label(&mut self, label: &str) {
        self.emit(0xCD);
        self.emit_label_ref(label);
    }

    fn ret(&mut self) { self.emit(0xC9); }
    fn ret_z(&mut self) { self.emit(0xC8); }

    fn ld_hl_nn(&mut self, nn: u16) { self.emit(0x21); self.emit16(nn); }
    fn ld_de_nn(&mut self, nn: u16) { self.emit(0x11); self.emit16(nn); }
    fn ld_bc_nn(&mut self, nn: u16) { self.emit(0x01); self.emit16(nn); }
    fn ld_hl_label(&mut self, label: &str) { self.emit(0x21); self.emit_label_ref(label); }

    fn ld_a_n(&mut self, n: u8) { self.emit(0x3E); self.emit(n); }
    fn ld_b_n(&mut self, n: u8) { self.emit(0x06); self.emit(n); }
    fn ld_c_n(&mut self, n: u8) { self.emit(0x0E); self.emit(n); }
    fn ld_d_n(&mut self, n: u8) { self.emit(0x16); self.emit(n); }
    fn ld_e_n(&mut self, n: u8) { self.emit(0x1E); self.emit(n); }
    fn ld_h_n(&mut self, n: u8) { self.emit(0x26); self.emit(n); }
    fn ld_l_n(&mut self, n: u8) { self.emit(0x2E); self.emit(n); }

    fn ld_a_hl(&mut self) { self.emit(0x7E); }
    fn ld_hl_a(&mut self) { self.emit(0x77); }
    fn ld_a_de(&mut self) { self.emit(0x1A); }
    fn ld_de_a(&mut self) { self.emit(0x12); }
    fn ld_a_b(&mut self) { self.emit(0x78); }
    fn ld_a_c(&mut self) { self.emit(0x79); }
    fn ld_a_d(&mut self) { self.emit(0x7A); }
    fn ld_a_e(&mut self) { self.emit(0x7B); }
    fn ld_a_l(&mut self) { self.emit(0x7D); }
    fn ld_a_h(&mut self) { self.emit(0x7C); }
    fn ld_l_a(&mut self) { self.emit(0x6F); }
    fn ld_h_a(&mut self) { self.emit(0x67); }
    fn ld_e_a(&mut self) { self.emit(0x5F); }
    fn ld_d_a(&mut self) { self.emit(0x57); }
    fn ld_b_a(&mut self) { self.emit(0x47); }
    fn ld_c_a(&mut self) { self.emit(0x4F); }
    fn ld_e_hl(&mut self) { self.emit(0x5E); }
    fn ld_d_hl(&mut self) { self.emit(0x56); }
    fn ld_l_e(&mut self) { self.emit(0x6B); }
    fn ld_h_d(&mut self) { self.emit(0x62); }
    fn ld_h_hl(&mut self) { self.emit(0x66); }

    fn ld_a_mem(&mut self, addr: u16) { self.emit(0x3A); self.emit16(addr); }
    fn ld_mem_a(&mut self, addr: u16) { self.emit(0x32); self.emit16(addr); }

    fn inc_hl(&mut self) { self.emit(0x23); }
    fn inc_de(&mut self) { self.emit(0x13); }
    fn inc_bc(&mut self) { self.emit(0x03); }
    fn inc_a(&mut self) { self.emit(0x3C); }
    fn inc_b(&mut self) { self.emit(0x04); }
    fn inc_hl_ind(&mut self) { self.emit(0x34); }

    fn dec_a(&mut self) { self.emit(0x3D); }
    fn dec_b(&mut self) { self.emit(0x05); }
    fn dec_c(&mut self) { self.emit(0x0D); }
    fn dec_d(&mut self) { self.emit(0x15); }
    fn dec_e(&mut self) { self.emit(0x1D); }
    fn dec_hl(&mut self) { self.emit(0x2B); }
    fn dec_bc(&mut self) { self.emit(0x0B); }

    fn add_hl_de(&mut self) { self.emit(0x19); }
    fn add_hl_hl(&mut self) { self.emit(0x29); }
    fn add_a_n(&mut self, n: u8) { self.emit(0xC6); self.emit(n); }
    fn add_a_hl(&mut self) { self.emit(0x86); }

    fn sbc_hl_de(&mut self) { self.emit(0xED); self.emit(0x52); }

    fn sub_n(&mut self, n: u8) { self.emit(0xD6); self.emit(n); }
    fn sub_hl(&mut self) { self.emit(0x96); }

    fn and_n(&mut self, n: u8) { self.emit(0xE6); self.emit(n); }
    fn and_a_b(&mut self) { self.emit(0xA0); }
    fn and_a_c(&mut self) { self.emit(0xA1); }
    fn and_a_d(&mut self) { self.emit(0xA2); }
    fn and_a_e(&mut self) { self.emit(0xA3); }
    fn and_hl(&mut self) { self.emit(0xA6); }

    fn or_a(&mut self) { self.emit(0xB7); }
    fn or_c(&mut self) { self.emit(0xB1); }
    fn or_hl(&mut self) { self.emit(0xB6); }

    fn xor_a(&mut self) { self.emit(0xAF); }
    fn xor_h(&mut self) { self.emit(0xAC); }
    fn xor_hl(&mut self) { self.emit(0xAE); }

    fn cp_n(&mut self, n: u8) { self.emit(0xFE); self.emit(n); }
    fn cp_hl(&mut self) { self.emit(0xBE); }

    fn push_af(&mut self) { self.emit(0xF5); }
    fn push_hl(&mut self) { self.emit(0xE5); }
    fn push_de(&mut self) { self.emit(0xD5); }
    fn pop_af(&mut self) { self.emit(0xF1); }
    fn pop_hl(&mut self) { self.emit(0xE1); }
    fn pop_de(&mut self) { self.emit(0xD1); }

    fn ex_de_hl(&mut self) { self.emit(0xEB); }

    fn out_n_a(&mut self, port: u8) { self.emit(0xD3); self.emit(port); }
    fn in_a_n(&mut self, port: u8) { self.emit(0xDB); self.emit(port); }
}
