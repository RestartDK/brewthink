#!/usr/bin/env python3
"""A limited direct-call regression gate, never a whole-program stack bound."""
import argparse
from collections import deque
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
from typing import NamedTuple

sys.path.insert(0, str(Path(__file__).resolve().parent))
import reader_frame_evidence as evidence


TASK = "brewthink::x4::reader_app::__reader_app_task_task::__reader_app_task_task_inner_function::{closure#0}"
LIBRARY = "brewthink::x4::reader_app::load_library"
EFFECT = "brewthink::x4::reader_app::run_effect"
RESERVE_BYTES = 8 * 1024
SCOPES = (
    "brewthink::x4::reader_app::",
    "brewthink::reader_orchestration::",
    "brewthink::storage::book_resume::",
    "brewthink::device_epub::",
    "brewthink::zip_stream::",
    "brewthink::scratch::",
    "brewthink::bounded_layout::",
)
LIMITATIONS = [
    "Only emitted symbols in SCOPES and the reader task poll are selected.",
    "Direct edges stop at unmeasured callees; paths through them back into selected code are not followed.",
    "Calls assume ABI-balanced SP. Frame propagation discards constants across calls; target inference retains ABI-saved values. Unresolved external calls/tails remain unmeasured.",
    "Illegal-instruction and breakpoint traps end local paths; their exception handlers remain unmeasured.",
    "Local dispatch relies on pinned, same-artifact compiler provenance and well-defined source execution, not corrupted coroutine state. Unknown assembly and non-immediate SP changes are rejected.",
    "Recursion is rejected only inside the selected direct graph; recursion beyond its frontier is unmeasured.",
    "Interrupt nesting, executor callers, other tasks, assembly/ROM and dynamic stack use are not bounded.",
    "Absent/inlined symbols have no independent frame measurement; no absence is treated as proof of inlining.",
    "Duplicate demangled names are address-qualified; ambiguous call targets stay on the unmeasured frontier.",
    "Selected direct paths sum full frames, including tail transfers, and can overcount mutually exclusive work.",
    "The 8192-byte reserve is an unchanged allowance, not a measurement of omitted paths.",
]


def instructions(body):
    parsed = []
    for line in body:
        match = re.match(r"\s*([0-9a-f]+):\s+([a-z][a-z0-9.]*)\s*(.*)", line)
        if match:
            address, opcode, operands = match.groups()
            annotation = re.search(r" <(.+)>$", operands)
            target = annotation[1] if annotation else None
            operands = operands[:annotation.start()] if annotation else operands
            parsed.append((int(address, 16), opcode, [part.strip() for part in operands.split(",")], target))
        elif re.match(r"\s*[0-9a-f]+:", line):
            raise ValueError(f"unparsed instruction: {line}")
    if not parsed:
        raise ValueError("empty disassembly body")
    return parsed


TRAPS = {"unimp", "ebreak"}
BRANCHES = {"beq", "bne", "blt", "bge", "bltu", "bgeu", "beqz", "bnez", "blez", "bgez", "bltz", "bgtz", "bgt", "ble", "bgtu", "bleu"}
NO_DESTINATION = {"sw", "sh", "sb", "fsw", "fsd", "fence", "fence.i", "nop", "csrc", "csrs", "csrw", "csrci", "csrsi", "csrwi"}
REGISTER_DESTINATION = {
    "add", "addi", "and", "andi", "auipc", "div", "divu", "lb", "lbu", "lh", "lhu",
    "li", "lui", "lw", "mul", "mulh", "mulhsu", "mulhu", "mv", "neg", "not", "or", "ori",
    "rem", "remu", "seqz", "sgtz", "sll", "slli", "slt", "slti", "sltiu", "sltu", "sltz",
    "snez", "sra", "srai", "srl", "srli", "sub", "xor", "xori", "zext.b", "zext.h", "sext.b", "sext.h",
    "csrr", "csrrc", "csrrci", "csrrs", "csrrsi", "csrrw", "csrrwi",
}


def control_flow(opcode):
    return opcode in {"jal", "jalr", "j", "jr", "ret", "call", "tail"} | BRANCHES | TRAPS


def is_call(opcode, args):
    return opcode == "call" or (opcode in {"jal", "jalr"} and (len(args) == 1 or args[0] in {"ra", "x1"}))


def frame_cfg(parsed, transfers=None):
    addresses = {address: i for i, (address, _, _, _) in enumerate(parsed)}
    if len(addresses) != len(parsed):
        raise ValueError("duplicate instruction address")
    branch_targets = {
        int(arg, 16)
        for _, opcode, args, _ in parsed if control_flow(opcode)
        for arg in args if re.fullmatch(r"0x[0-9a-f]+", arg)
    }
    successors = []
    unresolved = set()
    for i, (address, opcode, args, _) in enumerate(parsed):
        following = [i + 1] if i + 1 < len(parsed) else []
        if not control_flow(opcode) and opcode not in NO_DESTINATION | REGISTER_DESTINATION:
            raise ValueError(f"unsupported instruction at {address:x}: {opcode}")
        if opcode in {"jal", "jalr"} and len(args) > 1 and args[0] not in {"ra", "x1", "zero", "x0"}:
            raise ValueError(f"unsupported link register at {address:x}")
        if is_call(opcode, args):
            successors.append(following)
        elif opcode in {"ret"} | TRAPS or (opcode == "jr" and args == ["ra"]):
            successors.append([])
        elif control_flow(opcode):
            destination = None
            if opcode in BRANCHES | {"j", "jal", "tail"}:
                destination = next((int(arg, 16) for arg in args if re.fullmatch(r"0x[0-9a-f]+", arg)), None)
            relative = re.fullmatch(r"(-?0x[0-9a-f]+)\((\w+)\)", args[-1])
            if destination is None and relative and i and address not in branch_targets:
                previous_address, previous_opcode, previous_args, _ = parsed[i - 1]
                if previous_opcode == "auipc" and previous_args[0] == relative[2]:
                    displacement = int(previous_args[1], 0) << 12
                    displacement = (displacement + (1 << 31)) % (1 << 32) - (1 << 31)
                    destination = (previous_address + displacement + int(relative[1], 0)) & 0xfffffffe
            destinations = {destination} if destination is not None else (transfers or {}).get(address)
            branch = []
            if destinations is None:
                if opcode in BRANCHES or opcode == "j":
                    raise ValueError(f"unknown branch target at {address:x}")
                unresolved.add(i)
            else:
                for destination in sorted(destinations):
                    if destination in addresses:
                        branch.append(addresses[destination])
                    elif parsed[0][0] <= destination <= parsed[-1][0]:
                        raise ValueError(f"branch into an unparsed instruction at {address:x}")
            successors.append(branch + following if opcode in BRANCHES else branch)
        else:
            successors.append(following)
    return successors, unresolved


def register_constants(opcode, args, registers):
    if is_call(opcode, args):
        return {"zero": 0}
    if control_flow(opcode) or opcode in NO_DESTINATION:
        return registers.copy()
    result = registers.copy()
    value = None
    if opcode == "li":
        value = int(args[1], 0)
    elif opcode == "lui":
        value = int(args[1], 0) << 12
    elif opcode in {"addi", "add", "sub", "mv"}:
        left = registers.get(args[1])
        right = 0 if opcode == "mv" else (int(args[2], 0) if opcode == "addi" else registers.get(args[2]))
        if left is not None and right is not None:
            value = left - right if opcode == "sub" else left + right
    if args[0] not in {"zero", "x0", "sp", "x2"}:
        if value is None:
            result.pop(args[0], None)
        else:
            result[args[0]] = (value + (1 << 31)) % (1 << 32) - (1 << 31)
    return result


VALUE_LIMIT = 1024
WORD_MASK = (1 << 32) - 1


def readonly_sections(elf):
    if len(elf) < 52 or elf[:7] != b"\x7fELF\x01\x01\x01" or struct.unpack_from("<H", elf, 18)[0] != 243:
        raise ValueError("expected a complete little-endian ELF32 RISC-V header")
    offset = struct.unpack_from("<I", elf, 32)[0]
    size, count = struct.unpack_from("<HH", elf, 46)
    if size != 40 or not count or offset + size * count > len(elf):
        raise ValueError("missing or truncated ELF section table")
    sections = []
    for index in range(count):
        _, kind, flags, address, start, length, *_ = struct.unpack_from("<10I", elf, offset + index * size)
        if kind != 1 or not flags & 2 or flags & 5 or not length:
            continue
        if start + length > len(elf) or address + length > 1 << 32:
            raise ValueError("truncated or overflowing readonly ELF section")
        if any(address < base + len(data) and base < address + length for base, data in sections):
            raise ValueError("overlapping readonly ELF sections")
        sections.append((address, elf[start:start + length]))
    if not sections:
        raise ValueError("ELF has no immutable allocated data")
    return sections


def finite(values):
    result = set()
    for value in values:
        result.add(value & WORD_MASK)
        if len(result) > VALUE_LIMIT:
            return None
    return frozenset(result)


def loaded_values(addresses, width, readonly):
    if addresses is None:
        return None
    values = set()
    for address in addresses:
        section = next(((base, data) for base, data in readonly if base <= address and address + width <= base + len(data)), None)
        if section is None or address % width:
            return None
        base, data = section
        values.add(int.from_bytes(data[address - base:address - base + width], "little"))
    return finite(values)


def register_values(address, opcode, args, registers, readonly):
    if is_call(opcode, args):
        return {name: values for name, values in registers.items() if name == "zero" or re.fullmatch(r"s(?:[0-9]|1[01])", name)}
    if control_flow(opcode) or opcode in NO_DESTINATION:
        return registers.copy()
    result = registers.copy()
    value = None
    if opcode == "li":
        value = finite([int(args[1], 0)])
    elif opcode in {"lui", "auipc"}:
        value = finite([(int(args[1], 0) << 12) + (address if opcode == "auipc" else 0)])
    elif opcode in {"lw", "lbu", "lb", "lhu", "lh"}:
        operand = re.fullmatch(r"(-?0x[0-9a-f]+|[0-9]+)\((\w+)\)", args[1])
        if operand:
            base = registers.get(operand[2])
            addresses = finite(v + int(operand[1], 0) for v in base) if base is not None else None
            width = {"lw": 4, "lbu": 1, "lb": 1, "lhu": 2, "lh": 2}[opcode]
            value = loaded_values(addresses, width, readonly)
            if value is not None and opcode in {"lb", "lh"}:
                sign = 1 << (width * 8 - 1)
                value = finite((v ^ sign) - sign for v in value)
        if value is None and opcode in {"lbu", "lb"}:
            value = finite(range(256) if opcode == "lbu" else range(-128, 128))
    elif opcode in {"sltiu", "sltu", "slti", "slt", "seqz", "snez"}:
        value = frozenset({0, 1})
    elif opcode in {"add", "addi", "sub", "mv", "and", "andi", "slli", "srli", "zext.b"}:
        left = registers.get(args[1])
        if opcode == "zext.b":
            value = finite(v & 255 for v in left) if left is not None else finite(range(256))
        elif opcode == "mv":
            value = left
        else:
            right = finite([int(args[2], 0)]) if opcode in {"addi", "andi", "slli", "srli"} else registers.get(args[2])
            operations = {"add": lambda a, b: a + b, "addi": lambda a, b: a + b,
                          "sub": lambda a, b: a - b, "and": lambda a, b: a & b,
                          "andi": lambda a, b: a & b, "slli": lambda a, b: a << (b & 31),
                          "srli": lambda a, b: a >> (b & 31)}
            if left is not None and right is not None:
                value = finite(operations[opcode](a, b) for a in left for b in right)
            elif opcode == "andi" and 0 <= int(args[2], 0) < VALUE_LIMIT:
                value = finite(range(int(args[2], 0) + 1))
    if args[0] not in {"zero", "x0", "sp", "x2"}:
        if value is None:
            result.pop(args[0], None)
        else:
            result[args[0]] = value
    return result


def branch_values(opcode, args, registers, taken):
    result = registers.copy()
    if opcode not in {"bltu", "bgeu"}:
        return result
    less = taken == (opcode == "bltu")
    left, right = (args[0], args[1]) if less else (args[1], args[0])
    inclusive = not less
    lhs, rhs = result.get(left), result.get(right)
    if rhs is not None:
        upper = max(rhs) + int(inclusive)
        if lhs is None and upper <= VALUE_LIMIT:
            lhs = frozenset(range(upper))
        if lhs is not None:
            result[left] = frozenset(value for value in lhs if value < upper)
    if lhs:
        lower = min(lhs) - int(inclusive)
        if rhs is not None:
            result[right] = frozenset(value for value in rhs if value > lower)
    return None if any(not values for values in result.values()) else result


class TransferEvidence(NamedTuple):
    complete: dict[int, frozenset[int]]
    discovered: dict[int, frozenset[int]]


def resolve_transfers(parsed, readonly):
    transfers = {}
    while True:
        successors, unresolved = frame_cfg(parsed, transfers)
        environments = {0: {"zero": frozenset({0})}}
        pending = deque([0])
        queued = {0}
        while pending:
            i = pending.popleft()
            queued.remove(i)
            address, opcode, args, _ = parsed[i]
            outgoing = register_values(address, opcode, args, environments[i], readonly)
            for child in successors[i]:
                values = outgoing if successors[i].count(child) > 1 else branch_values(opcode, args, outgoing, child != i + 1)
                if values is None:
                    continue
                previous = environments.get(child)
                joined = values if previous is None else {
                    key: union for key in previous.keys() & values.keys()
                    if (union := finite(previous[key] | values[key])) is not None
                }
                if previous != joined:
                    environments[child] = joined
                    if child not in queued:
                        pending.append(child)
                        queued.add(child)
        complete = {}
        changed = False
        for i, (address, opcode, args, _) in enumerate(parsed):
            if i not in environments or (i not in unresolved and address not in transfers):
                continue
            operand = re.fullmatch(r"(-?0x[0-9a-f]+|[0-9]+)\((\w+)\)", args[-1])
            register = operand[2] if operand else args[-1]
            values = environments[i].get(register)
            if values is None:
                continue
            offset = int(operand[1], 0) if operand else 0
            targets = frozenset((value + offset) & 0xfffffffe for value in values)
            complete[address] = targets
            joined = transfers.get(address, frozenset()) | targets
            if joined != transfers.get(address):
                transfers[address] = joined
                changed = True
        if not changed:
            return TransferEvidence(complete, transfers)


def stack_frame(body, transfers=None):
    parsed = instructions(body)
    successors, unresolved = frame_cfg(parsed, transfers)
    constants = {0: {"zero": 0}}
    pending = deque([0])
    while pending:
        i = pending.popleft()
        _, opcode, args, _ = parsed[i]
        outgoing = register_constants(opcode, args, constants[i])
        for child in successors[i]:
            previous = constants.get(child)
            joined = outgoing if previous is None else {key: value for key, value in previous.items() if outgoing.get(key) == value}
            if previous != joined:
                constants[child] = joined
                pending.append(child)

    deltas = [0] * len(parsed)
    for i, (address, opcode, args, _) in enumerate(parsed):
        if opcode in NO_DESTINATION or args[0] not in {"sp", "x2"}:
            continue
        if i not in constants:
            raise ValueError(f"unreachable stack write has no frame proof at {address:x}")
        delta = None
        if opcode in {"addi", "add", "sub", "mv"} and args[1] == "sp":
            delta = 0 if opcode == "mv" else (int(args[2], 0) if opcode == "addi" else constants[i].get(args[2]))
            if delta is not None and opcode == "sub":
                delta = -delta
        if delta is None:
            raise ValueError(f"unknown stack adjustment at {address:x}")
        deltas[i] = -delta

    depths = {0: (0, 0)}
    maximum = 0
    for _ in range(len(parsed)):
        changed = False
        for i in range(len(parsed)):
            if i not in depths:
                continue
            low, high = (value + deltas[i] for value in depths[i])
            maximum = max(maximum, high)
            for child in successors[i]:
                previous = depths.get(child)
                joined = (low, high) if previous is None else (min(previous[0], low), max(previous[1], high))
                if previous != joined:
                    depths[child] = joined
                    changed = True
        if not changed:
            break
    else:
        raise ValueError("unbounded or inconsistent stack-depth cycle")
    for i, (low, high) in depths.items():
        address = parsed[i][0]
        if low + deltas[i] < 0 or high + deltas[i] >= 1 << 31:
            raise ValueError(f"stack depth outside supported entry-relative range at {address:x}")
        if i in unresolved and (low, high) != (0, 0):
            raise ValueError(f"unresolved in-frame transfer at {address:x}; internal targets require proof")
    return maximum


def function_extents(elf):
    readonly_sections(elf)
    section_offset = struct.unpack_from("<I", elf, 32)[0]
    section_count = struct.unpack_from("<H", elf, 48)[0]
    sections = [struct.unpack_from("<10I", elf, section_offset + index * 40) for index in range(section_count)]
    extents = {}
    for _, kind, _, _, offset, length, _, _, _, entry_size in sections:
        if kind != 2:
            continue
        if entry_size != 16 or length % 16 or offset + length > len(elf):
            raise ValueError("malformed ELF symbol table")
        for position in range(offset, offset + length, 16):
            _, address, size, info, _, section = struct.unpack_from("<IIIBBH", elf, position)
            if info & 15 != 2 or section == 0 or section >= 0xff00:
                continue
            if section >= len(sections):
                raise ValueError("function has an invalid ELF section")
            _, section_kind, flags, base, start, count, *_ = sections[section]
            if section_kind != 1 or flags & 6 != 6 or start + count > len(elf):
                raise ValueError("function lacks complete executable storage")
            if not base <= address <= address + size <= base + count <= 1 << 32:
                raise ValueError("function exceeds its executable section")
            previous = extents.get(address, size)
            if previous and size and previous != size:
                raise ValueError("conflicting function extents at one address")
            extents[address] = max(previous, size)
    if not extents:
        raise ValueError("ELF has no defined function symbols")
    return extents


def functions_from(disassembly, extents=None):
    functions = {}
    body = None
    addresses = {}
    extents = extents or {}
    owner_start = owner_end = None
    for line in disassembly.splitlines():
        header = re.fullmatch(r"([0-9a-f]+) <(.+)>:", line)
        if header:
            address, name = header.groups()
            start = int(address, 16)
            if owner_end is not None and owner_start < start < owner_end and start not in extents:
                continue
            owner_start = start
            owner_end = start + extents[start] if start in extents else None
            if name in addresses:
                if name in functions:
                    functions[f"{name} [0x{addresses[name]}]"] = functions.pop(name)
                name = f"{name} [0x{address}]"
            else:
                addresses[name] = address
            if name in functions:
                raise ValueError(f"duplicate symbol and address: {name}")
            body = []
            functions[name] = body
        elif body is not None:
            body.append(line)
    return functions


def reader_symbols(functions):
    polls = [name for name in functions if "TaskStorage<" + TASK in name and name.endswith("::poll")]
    if len(polls) != 1:
        raise ValueError("reader task poll symbol is missing or ambiguous")
    for name in (LIBRARY, EFFECT):
        if name not in functions:
            raise ValueError(f"required symbol missing; review coverage rather than assuming inlining: {name}")
    return polls[0]


def reader_frames(disassembly, transfers=None):
    functions = functions_from(disassembly)
    poll = reader_symbols(functions)
    transfers = transfers or {}
    task_bytes = stack_frame(functions[poll], transfers.get(poll))
    if TASK in functions:
        task_bytes += stack_frame(functions[TASK], transfers.get(TASK))
    return {"task": task_bytes, "library": stack_frame(functions[LIBRARY], transfers.get(LIBRARY)), "effects": stack_frame(functions[EFFECT], transfers.get(EFFECT))}


def check(frames, available):
    required = frames["task"] + max(frames["library"], frames["effects"]) + RESERVE_BYTES
    if frames["task"] > 4096 or required > available:
        raise ValueError(f"reader stack budget exceeded: frames={frames}, required={required}, available={available}")
    return required


def direct_graph(functions, selected, transfers=None, discovered=None):
    edges = {name: set() for name in selected}
    frontier = []
    entries = {instructions(functions[name])[0][0]: name for name in selected}
    transfers = transfers or {}
    for name in sorted(selected):
        for address, opcode, args, target in instructions(functions[name]):
            if not control_flow(opcode) or opcode == "ret" or (opcode == "jr" and args == ["ra"]):
                continue
            if opcode in BRANCHES | {"j", "jal", "tail"}:
                destination = next((int(arg, 16) for arg in args if re.fullmatch(r"0x[0-9a-f]+", arg)), None)
                body = instructions(functions[name])
                if destination is not None:
                    if body[0][0] <= destination <= body[-1][0] and not is_call(opcode, args):
                        continue
                    if destination in entries:
                        edges[name].add(entries[destination])
                        continue
            if target is None and address in transfers.get(name, {}):
                body = instructions(functions[name])
                for destination in sorted(transfers[name][address]):
                    if body[0][0] <= destination <= body[-1][0]:
                        continue
                    callee = entries.get(destination)
                    if callee in selected:
                        edges[name].add(callee)
                    else:
                        frontier.append({"caller": name, "address": hex(address), "kind": "computed-external-target", "target": hex(destination), "symbol": callee})
                continue
            if target is None:
                kind = "exception-transfer" if opcode in TRAPS else "unresolved-transfer"
                frontier.append({"caller": name, "address": hex(address), "kind": kind, "instruction": opcode + " " + ", ".join(args)})
                continue
            callee = re.sub(r"\+0x[0-9a-f]+$", "", target)
            if callee == name.split(" [0x")[0] and not is_call(opcode, args):
                continue
            if callee in selected and target == callee:
                edges[name].add(callee)
            else:
                frontier.append({"caller": name, "address": hex(address), "kind": "unmeasured-target", "target": target})
    for name, sites in (discovered or {}).items():
        body = instructions(functions[name])
        for targets in sites.values():
            for target in targets:
                if body[0][0] <= target <= body[-1][0]:
                    continue
                if target in entries:
                    edges[name].add(entries[target])
    return edges, frontier


def longest_path(root, edges, frames):
    active = set()
    memo = {}

    def visit(name):
        if name in active:
            raise ValueError(f"recursive selected call graph: {name}")
        if name in memo:
            return memo[name]
        active.add(name)
        children = [visit(child) for child in sorted(edges[name])]
        child_bytes, path = max(children, default=(0, []), key=lambda item: item[0])
        active.remove(name)
        memo[name] = frames[name] + child_bytes, [name] + path
        return memo[name]

    for name in sorted(edges):
        visit(name)
    return memo[root]


def analyze(disassembly, available, readonly=None, compiler=None, extents=None):
    functions = functions_from(disassembly, extents)
    poll = reader_symbols(functions)
    selected = {name for name in functions if name.lstrip("<").startswith(SCOPES)} | {poll}
    if compiler is not None and set(compiler) != selected:
        raise ValueError("compiler coverage differs from the complete selected scope")
    frames = {}
    local_dispatches = {}
    unsupported = {}
    transfers = {}
    discovered = {}
    for name in sorted(selected):
        try:
            resolution = resolve_transfers(instructions(functions[name]), readonly) if readonly is not None else TransferEvidence({}, {})
            transfers[name], discovered[name] = resolution
            if compiler is None:
                frames[name] = stack_frame(functions[name], transfers[name])
            else:
                local_dispatches[name] = evidence.corroborate(sys.modules[__name__], functions[name], compiler[name], transfers[name])
                frames[name] = compiler[name]["size"]
        except ValueError as error:
            unsupported[name] = str(error)
    edges, frontier = direct_graph(functions, selected, transfers, discovered)
    if compiler is not None:
        for name in selected:
            edges[name].update(compiler[name]["callees"])
        frontier = [site for site in frontier if int(site["address"], 16) not in local_dispatches.get(site["caller"], set())]
    reachable = set()
    pending = [poll]
    while pending:
        name = pending.pop()
        if name not in reachable:
            reachable.add(name)
            pending.extend(edges[name])
    report = {
        "verdict": "BLOCKED_UNPROVEN", "whole_program_bound": False,
        "available_bytes": available, "reserve_bytes": RESERVE_BYTES,
        "entry_frames": None, "entry_required_bytes": None,
        "selected_path_bytes": None, "selected_path": None,
        "accounted_bytes_with_reserve": None,
        "selected_symbol_count": len(selected), "unsupported_frames": unsupported,
        "frames": frames, "edges": {name: sorted(children) for name, children in sorted(edges.items())},
        "computed_transfers": {name: {hex(address): [hex(target) for target in sorted(targets)] for address, targets in sorted(sites.items())} for name, sites in sorted(transfers.items()) if sites},
        "discovered_transfer_candidates": {name: {hex(address): [hex(target) for target in sorted(targets)] for address, targets in sorted(sites.items())} for name, sites in sorted(discovered.items()) if sites},
        "compiler_local_dispatches": {name: [hex(address) for address in sorted(sites)] for name, sites in local_dispatches.items() if sites},
        "frontier": frontier, "selected_not_reached_by_direct_edges": sorted(selected - reachable),
        "unselected_symbol_count": len(functions) - len(selected),
        "task_body_symbol": "measured" if TASK in functions else "absent; no independent measurement",
        "scopes": SCOPES, "limitations": LIMITATIONS,
    }
    if unsupported:
        return report
    entries = {"task": frames[poll] + frames.get(TASK, 0), "library": frames[LIBRARY], "effects": frames[EFFECT]}
    entry_required = entries["task"] + max(entries["library"], entries["effects"]) + RESERVE_BYTES
    path_bytes, path = longest_path(poll, edges, frames)
    accounted = max(entry_required, path_bytes + RESERVE_BYTES, max(frames.values()) + RESERVE_BYTES)
    report.update(
        verdict="BLOCKED_BUDGET" if entries["task"] > 4096 or accounted > available else "PASS_LIMITED",
        entry_frames=entries, entry_required_bytes=entry_required,
        selected_path_bytes=path_bytes, selected_path=path,
        accounted_bytes_with_reserve=accounted,
    )
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("elf", type=Path)
    parser.add_argument("--evidence", type=Path, required=True, help="fresh two-link reader-memory evidence directory")
    parser.add_argument("--report", type=Path, help="write full JSON coverage and ELF identity")
    parser.add_argument("--require-complete", action="store_true", help="reject this limited evidence as a whole-program proof")
    arguments = parser.parse_args()
    disassembly = subprocess.check_output(["llvm-objdump", "-d", "--demangle", "--no-show-raw-insn", str(arguments.elf)], text=True)
    sizes = subprocess.check_output(["llvm-size", "-A", str(arguments.elf)], text=True)
    matches = re.findall(r"^\.stack\s+(\d+)", sizes, re.MULTILINE)
    if len(matches) != 1 or int(matches[0]) <= 0:
        raise ValueError("ELF must have exactly one nonempty .stack section")
    import importlib.util
    spec = importlib.util.spec_from_file_location("reader_memory", Path(__file__).with_name("check-reader-memory.py"))
    memory = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(memory)
    bundle = memory.validate_bundle(arguments.evidence, arguments.elf)
    extents = function_extents(arguments.elf.read_bytes())
    functions = functions_from(disassembly, extents)
    inventory = evidence.symbol_inventory(sys.modules[__name__], arguments.elf.read_bytes(), functions,
        subprocess.check_output(["llvm-nm", "--defined-only", "--print-size", str(arguments.elf)], text=True))
    if inventory != bundle["symbols"]:
        raise ValueError("selected raw symbols/extents differ from linked evidence")
    with (arguments.evidence / "frames.log").open() as log:
        records = evidence.frame_records(log, inventory)
    with (arguments.evidence / "machine.log").open() as log:
        machines = evidence.machine_records(log, inventory)
    compiler = {}
    for raw, symbol in inventory.items():
        proof = evidence.machine_frame(machines[raw], records[raw])
        proof["callees"] = [inventory[callee]["name"] for callee in proof["calls"] if callee in inventory]
        compiler[symbol["name"]] = proof
    report = analyze(disassembly, int(matches[0]), readonly=readonly_sections(arguments.elf.read_bytes()), compiler=compiler, extents=extents)
    report["compiler_frames"] = {name: {key: value for key, value in proof.items() if key != "sp_writes"} for name, proof in compiler.items()}
    report["evidence_inputs_sha256"] = hashlib.sha256((arguments.evidence / "inputs.json").read_bytes()).hexdigest()
    report["elf"] = str(arguments.elf.resolve())
    report["elf_sha256"] = hashlib.sha256(arguments.elf.read_bytes()).hexdigest()
    report["objdump_version"] = subprocess.check_output(["llvm-objdump", "--version"], text=True).splitlines()[0]
    if arguments.report:
        arguments.report.write_text(json.dumps(report, indent=2) + "\n")
    print(f"reader-stack {report['verdict']} accounted={report['accounted_bytes_with_reserve']} available={report['available_bytes']} reserve={RESERVE_BYTES} selected_frames={len(report['frames'])}/{report['selected_symbol_count']} frontier_sites={len(report['frontier'])}")
    if report["selected_path"] is not None:
        print("selected direct path: " + " -> ".join(report["selected_path"]))
    for name, error in report["unsupported_frames"].items():
        print(f"UNPROVEN: {name}: {error}")
    for limitation in LIMITATIONS:
        print("LIMIT: " + limitation)
    if report["verdict"] != "PASS_LIMITED":
        raise SystemExit("reader stack proof blocked; no passing budget or whole-program bound")
    if arguments.require_complete:
        raise ValueError("whole-program proof unavailable; selected-frame evidence is incomplete")


if __name__ == "__main__":
    main()
