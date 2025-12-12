// kz80_chip8 - CHIP-8 to Z80 Static Compiler
// Compiles CHIP-8 ROMs to native Z80 code for RetroShield

mod chip8;
mod codegen;

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <input.ch8> [-o output.bin]", args[0]);
        eprintln!("       {} --disasm <input.ch8>", args[0]);
        process::exit(1);
    }

    // Check for disassembly mode
    if args[1] == "--disasm" || args[1] == "-d" {
        if args.len() < 3 {
            eprintln!("Usage: {} --disasm <input.ch8>", args[0]);
            process::exit(1);
        }
        let input_path = &args[2];
        match fs::read(input_path) {
            Ok(rom) => {
                chip8::disassemble(&rom);
            }
            Err(e) => {
                eprintln!("Error reading {}: {}", input_path, e);
                process::exit(1);
            }
        }
        return;
    }

    let input_path = &args[1];
    let output_path = if args.len() >= 4 && args[2] == "-o" {
        args[3].clone()
    } else {
        input_path.replace(".ch8", ".bin")
    };

    // Read CHIP-8 ROM
    let rom = match fs::read(input_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading {}: {}", input_path, e);
            process::exit(1);
        }
    };

    if rom.is_empty() {
        eprintln!("Error: ROM file is empty");
        process::exit(1);
    }

    // Compile to Z80
    let mut compiler = codegen::Compiler::new();
    match compiler.compile(&rom) {
        Ok(binary) => {
            if let Err(e) = fs::write(&output_path, &binary) {
                eprintln!("Error writing {}: {}", output_path, e);
                process::exit(1);
            }
            println!("Compiled {} -> {} ({} bytes)", input_path, output_path, binary.len());
        }
        Err(e) => {
            eprintln!("Compilation error: {}", e);
            process::exit(1);
        }
    }
}
