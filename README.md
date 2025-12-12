# kz80_chip8

A CHIP-8 to Z80 static recompiler for RetroShield hardware.

## Overview

kz80_chip8 compiles CHIP-8 programs into native Z80 machine code, allowing classic CHIP-8 games and programs to run directly on Z80 hardware without an interpreter. The compiled output is a 32KB ROM image suitable for the RetroShield Z80.

## Features

- Static recompilation of CHIP-8 to native Z80 code
- Built-in CHIP-8 font sprites (0-F)
- Custom sprite support (embedded ROM data)
- ACIA serial output for display (64x32 text mode using `#` and space)
- Random number generation via LFSR
- Disassembler mode for examining CHIP-8 programs

## Building

```bash
cargo build --release
```

## Usage

### Compile a CHIP-8 ROM

```bash
./target/release/kz80_chip8 program.ch8 -o program.bin
```

### Disassemble a CHIP-8 ROM

```bash
./target/release/kz80_chip8 -d program.ch8
```

### Example

```bash
# Compile the IBM logo test ROM
./target/release/kz80_chip8 test/classic/ibm_logo.ch8 -o ibm.bin

# Run in the RetroShield emulator
../emulator/retroshield ibm.bin
```

## Memory Layout

The compiled Z80 code uses the following memory layout:

| Address Range | Description |
|---------------|-------------|
| 0x0000-0x00FF | RST vectors |
| 0x0100-0x7FFF | Compiled Z80 code + runtime (32KB ROM) |
| 0x8000-0x800F | CHIP-8 registers V0-VF |
| 0x8010-0x8011 | I register |
| 0x8012 | Stack pointer |
| 0x8013 | Delay timer |
| 0x8014 | Sound timer |
| 0x8016-0x8017 | RNG state |
| 0x8100-0x811F | CHIP-8 call stack |
| 0x8200-0x82FF | Display buffer (256 bytes) |
| 0x8300-0x834F | Font data |
| 0x8400-0xFFFF | General RAM |

## Supported CHIP-8 Instructions

- 00E0 - CLS (clear screen)
- 00EE - RET (return from subroutine)
- 1NNN - JP addr (jump)
- 2NNN - CALL addr (call subroutine)
- 3XNN - SE Vx, byte (skip if equal)
- 4XNN - SNE Vx, byte (skip if not equal)
- 5XY0 - SE Vx, Vy (skip if registers equal)
- 6XNN - LD Vx, byte (load immediate)
- 7XNN - ADD Vx, byte (add immediate)
- 8XY0 - LD Vx, Vy (copy register)
- 8XY1 - OR Vx, Vy
- 8XY2 - AND Vx, Vy
- 8XY3 - XOR Vx, Vy
- 8XY4 - ADD Vx, Vy (with carry)
- 8XY5 - SUB Vx, Vy (with borrow)
- 8XY6 - SHR Vx (shift right)
- 8XY7 - SUBN Vx, Vy (reverse subtract)
- 8XYE - SHL Vx (shift left)
- 9XY0 - SNE Vx, Vy (skip if not equal)
- ANNN - LD I, addr (set index register)
- BNNN - JP V0, addr (jump with offset)
- CXNN - RND Vx, byte (random)
- DXYN - DRW Vx, Vy, nibble (draw sprite)
- EX9E - SKP Vx (skip if key pressed)
- EXA1 - SKNP Vx (skip if key not pressed)
- FX07 - LD Vx, DT (get delay timer)
- FX0A - LD Vx, K (wait for key)
- FX15 - LD DT, Vx (set delay timer)
- FX18 - LD ST, Vx (set sound timer)
- FX1E - ADD I, Vx (add to index)
- FX29 - LD F, Vx (font sprite address)
- FX33 - LD B, Vx (BCD conversion)
- FX55 - LD [I], Vx (store registers)
- FX65 - LD Vx, [I] (load registers)

## Test ROMs

The `test/classic/` directory contains several classic CHIP-8 programs:

- `ibm_logo.ch8` - IBM logo display test
- `maze.ch8` - Random maze generator
- `pong.ch8` / `pong2.ch8` - Pong games
- `tetris.ch8` - Tetris
- `invaders.ch8` - Space Invaders

## License

BSD 3-Clause License. See [LICENSE](LICENSE) for details.
