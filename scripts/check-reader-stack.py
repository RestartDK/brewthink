#!/usr/bin/env python3
import argparse
import re
import subprocess


TASK = "brewthink::x4::reader_app::__reader_app_task_task::__reader_app_task_task_inner_function::{closure#0}"
LIBRARY = "brewthink::x4::reader_app::load_library"
EFFECT = "brewthink::x4::reader_app::run_effect"
RESERVE_BYTES = 8 * 1024


def stack_frame(body):
    registers = {"sp": 0, "zero": 0}
    minimum = 0
    for line in body:
        instruction = re.match(r"\s*[0-9a-f]+:\s+([a-z][a-z0-9.]*)\s*(.*)", line)
        if instruction is None:
            continue
        opcode, operands = instruction.groups()
        args = [part.strip() for part in operands.split(",")]
        if opcode in {"jal", "jalr", "j", "jr", "ret"} or opcode.startswith("b"):
            break
        if opcode == "lui":
            registers[args[0]] = int(args[1], 0) << 12
        elif opcode == "li":
            registers[args[0]] = int(args[1], 0)
        elif opcode in {"addi", "add", "sub", "mv"}:
            source = registers.get(args[1])
            right = 0 if opcode == "mv" else (
                int(args[2], 0) if opcode == "addi" else registers.get(args[2])
            )
            if source is None or right is None:
                if args[0] == "sp":
                    raise ValueError("unknown stack adjustment")
                registers.pop(args[0], None)
            else:
                registers[args[0]] = source - right if opcode == "sub" else source + right
        elif args[0] == "sp" and opcode not in {"sw", "sh", "sb"}:
            raise ValueError(f"unsupported stack instruction: {line}")
        elif opcode not in {"sw", "sh", "sb"}:
            registers.pop(args[0], None)
        minimum = min(minimum, registers["sp"])
    if minimum == 0:
        raise ValueError("no stack frame found; prologue:\n" + "\n".join(body[:24]))
    return -minimum


def reader_frames(disassembly):
    functions = {}
    body = []
    for line in disassembly.splitlines():
        header = re.match(r"[0-9a-f]+ <(.+)>:$", line)
        if header:
            body = []
            functions[header[1]] = body
        else:
            body.append(line)
    polls = [name for name in functions if "TaskStorage<" + TASK in name and name.endswith("::poll")]
    if len(polls) != 1:
        raise ValueError("reader task poll symbol is missing or ambiguous")
    task_bytes = stack_frame(functions[polls[0]])
    if TASK in functions:
        task_bytes += stack_frame(functions[TASK])
    return {"task": task_bytes, "library": stack_frame(functions[LIBRARY]), "effects": stack_frame(functions[EFFECT]) if EFFECT in functions else 0}


def check(frames, available):
    required = frames["task"] + max(frames["library"], frames["effects"]) + RESERVE_BYTES
    if frames["task"] > 4096 or required > available:
        raise ValueError(f"reader stack budget exceeded: frames={frames}, required={required}, available={available}")
    return required


def main():
    parser = argparse.ArgumentParser(description="Check reader entry frames against the linked X4 stack. This is not a whole-program stack analysis.")
    parser.add_argument("elf")
    arguments = parser.parse_args()
    disassembly = subprocess.check_output(["llvm-objdump", "-d", "--demangle", "--no-show-raw-insn", arguments.elf], text=True)
    sizes = subprocess.check_output(["llvm-size", "-A", arguments.elf], text=True)
    match = re.search(r"^\.stack\s+(\d+)", sizes, re.MULTILINE)
    if match is None:
        raise ValueError("ELF has no .stack section")
    available = int(match[1])
    frames = reader_frames(disassembly)
    required = check(frames, available)
    print(f"reader-stack frames={frames} reserve={RESERVE_BYTES} required={required} available={available}")


if __name__ == "__main__":
    main()
