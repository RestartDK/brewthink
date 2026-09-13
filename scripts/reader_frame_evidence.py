from collections import Counter, deque
import re


FRAME_NOTE = re.compile(r"note: .* prologepilog \(analysis\): (\d+) stack bytes in function '([^']+)'")
LAYOUT_NOTE = re.compile(r"note: .* stack-frame-layout \(analysis\):\s*")
SLOT = re.compile(r"Offset: \[SP([+-]\d+)\], Type: (Spill|Variable|Fixed), Align: (\d+), Size: (\d+)")
MACHINE_HEADER = re.compile(r"# Machine code for function ([^: ]+): (.+)")
MACHINE_END = re.compile(r"# End machine code for function ([^ ]+)\.")
MACHINE_PROPERTIES = "NoPHIs, TracksLiveness, NoVRegs, TiedOpsRewritten, TracksDebugUserValues"
ASM_TEMPLATES = {
    "csrrci ${0}, mstatus, 0x8": "regdef-ec",
    "csrrci ${0}, mstatus, 8": "regdef-ec",
    "csrrs x0, 0x300, ${0}": "reguse",
    "csrs mstatus, ${0}": "reguse",
}
MACHINE_OPS = set("""ADDI LW SW COPY LBU LUI PseudoMovAddr BEQ SB SUB BNE ANDI
SLLI OR ADD SH BLTU LHU SRLI PseudoRET BGEU AND SLTU PseudoTAILIndirect LH SLTIU
XOR PseudoBRIND PseudoCALLIndirect ORI BLT MUL XORI LB PseudoTAIL SRL UNIMP DIVU
BGE SRAI SLTI SLL MULHU PseudoBR PseudoCALL INLINEASM IMPLICIT_DEF KILL MEMBARRIER
CFI_INSTRUCTION DBG_VALUE DBG_VALUE_LIST""".split())
MACHINE_FLAGS = r"(?:(?:frame-setup|frame-destroy|nuw|nsw|disjoint|exact|samesign) )*"
MACHINE_BRANCHES = {"BEQ", "BNE", "BLT", "BGE", "BLTU", "BGEU", "PseudoBR"}
MACHINE_CALLS = {"PseudoCALL", "PseudoCALLIndirect"}
MACHINE_EXITS = {"PseudoRET", "PseudoTAIL", "PseudoTAILIndirect", "UNIMP"}
REGISTERS = "zero ra sp gp tp t0 t1 t2 s0 s1 a0 a1 a2 a3 a4 a5 a6 a7 s2 s3 s4 s5 s6 s7 s8 s9 s10 s11 t3 t4 t5 t6".split()


def lines_from(source):
    for line in source:
        if len(line) > 1_000_000 or '\x1b' in line:
            raise ValueError("oversized or decorated compiler record")
        yield line.rstrip('\n')


def insert_unique(records, name, value):
    if name in records:
        raise ValueError(f"duplicate compiler record: {name}")
    records[name] = value


def frame_records(source, selected):
    frames, layouts = {}, {}
    layout = None
    awaiting = False
    for line in lines_from(source):
        stripped = line.strip()
        if 'prologepilog (analysis)' in line:
            match = FRAME_NOTE.fullmatch(stripped)
            if not match:
                raise ValueError("malformed fixed-frame record")
            insert_unique(frames, match[2], int(match[1]))
        elif 'stack-frame-layout (analysis)' in line:
            if not LAYOUT_NOTE.fullmatch(stripped) or awaiting:
                raise ValueError("malformed layout record")
            awaiting, layout = True, None
        elif awaiting:
            match = re.fullmatch(r'Function: ([^ ]+)', stripped)
            if not match:
                raise ValueError("missing layout function")
            layout, awaiting = match[1], False
            insert_unique(layouts, layout, [])
        elif layout is not None:
            if not stripped:
                layout = None
            elif line.startswith('    ') and 'Offset:' not in line:
                if not re.fullmatch(r'\s+[^\n]+ @ [^\n]+', line):
                    raise ValueError("malformed layout source location")
            else:
                slot = SLOT.fullmatch(stripped)
                if not slot:
                    raise ValueError(f"unsupported frame slot: {stripped}")
                offset, kind, align, size = slot.groups()
                layouts[layout].append((int(offset), kind, int(align), int(size)))
    if awaiting:
        raise ValueError("truncated layout record")
    result = {}
    for raw in selected:
        if raw not in frames or raw not in layouts:
            raise ValueError(f"missing explicit frame/layout record: {raw}")
        size = frames[raw]
        if size % 16 or not 0 <= size < 1 << 31:
            raise ValueError("unsupported fixed scalar frame size")
        for offset, _, align, length in layouts[raw]:
            if align not in {1, 2, 4, 8, 16} or length < 0 or not -size <= offset <= -length:
                raise ValueError(f"unsupported slot alignment/extent: {raw}")
        result[raw] = {"size": size, "slots": layouts[raw]}
    return result


def machine_records(source, selected):
    records = {}
    name, body, selected_bytes = None, [], 0
    pass_marker = False
    for line in lines_from(source):
        if line == '# *** IR Dump Before RISC-V Assembly Printer (riscv-asm-printer) ***:':
            if name is not None:
                raise ValueError("interleaved machine records")
            pass_marker = True
        elif line.startswith('# Machine code for function '):
            match = MACHINE_HEADER.fullmatch(line)
            if not match or name is not None or not pass_marker:
                raise ValueError("malformed/unbound machine header")
            name = match[1]
            if name in selected and match[2] != MACHINE_PROPERTIES:
                raise ValueError("unsupported machine function properties")
            body, selected_bytes, pass_marker = [], 0, False
        elif line.startswith('# End machine code for function '):
            match = MACHINE_END.fullmatch(line)
            if not match or match[1] != name:
                raise ValueError("malformed/mismatched machine end")
            if name in selected:
                insert_unique(records, name, body)
            name = None
        elif name in selected:
            selected_bytes += len(line)
            if selected_bytes > 16_000_000:
                raise ValueError("selected machine record exceeds parser bound")
            body.append(line)
    if name is not None or set(records) != set(selected):
        raise ValueError("missing/truncated selected machine record")
    return records


def benign_assembly(text):
    match = re.fullmatch(r'INLINEASM &"([^"\n]*)" (.+)', text)
    if not match or match[1] not in ASM_TEMPLATES:
        raise ValueError("opaque inline assembly is not a fixed-frame proof")
    template, operands = match.groups()
    role = ASM_TEMPLATES[template]
    operand = re.search(r'\$0:\[([^]]+)\], ([^,]+)', operands)
    if not operand or operand[1] != role + ':GPRNoX0':
        raise ValueError("unsupported inline assembly operands")
    tokens = re.findall(r'\$([A-Za-z_][A-Za-z_0-9]*|\d+)', operands)
    if any(token not in {'vl', 'vtype'} and not token.isdecimal() and
           not re.fullmatch(r'x(?:[0-9]|[12][0-9]|3[01])', token) for token in tokens):
        raise ValueError("unknown inline assembly register or alias")
    registers = re.findall(r'\$x(\d+)\b', operands)
    if not registers or any(not 1 <= int(reg) <= 31 or int(reg) in {1, 2, 3, 4, 8} for reg in registers):
        raise ValueError("reserved register in inline assembly")
    if re.search(r'\$(?:sp|ra|fp|x2_\w+|x8_\w+)\b', operands):
        raise ValueError("stack/register alias in inline assembly")
    if re.search(r'\$(?:[1-9]\d*):\[(?!clobber\]|reguse tiedto:\$0\])', operands):
        raise ValueError("unexpected inline assembly operand")
    clobbers = re.findall(r'\[clobber\], implicit-def early-clobber (\$\w+)', operands)
    if any(reg not in {'$vtype', '$vl'} for reg in clobbers):
        raise ValueError("unsupported inline assembly clobber")
    return template


def machine_frame(body, record):
    blocks, tables, asm, sp_writes, indirect, calls = {}, {}, Counter(), Counter(), [], set()
    block = None
    object_ids = set()
    for line in body:
        if not line or line.startswith(';'):
            continue
        if line in {'Frame Objects:', 'save/restore points:', 'save points are empty', 'restore points are empty', 'Jump Tables:'}:
            continue
        if re.fullmatch(r'Function Live Ins: (?:\$x\d+(?: in %\d+)?(?:, )?)+', line):
            continue
        if line.startswith('  fi#'):
            match = re.fullmatch(r'  fi#(-?\d+): (?:dead|size=(\d+), align=(\d+), at location \[SP([+-]\d+)\])', line)
            if not match or match[1] in object_ids:
                raise ValueError("unsupported/duplicate machine frame object")
            object_ids.add(match[1])
            if match[2] is not None:
                length, align, offset = map(int, match.groups()[1:])
                if align not in {1, 2, 4, 8, 16} or not -record['size'] <= offset <= -length:
                    raise ValueError("unsupported machine object alignment/extent")
            continue
        if line.startswith('%jump-table.'):
            match = re.fullmatch(r'%jump-table\.(\d+): ((?:%bb\.\d+ ?)+)', line)
            if not match:
                raise ValueError("malformed machine jump table")
            insert_unique(tables, match[1], set(map(int, re.findall(r'%bb\.(\d+)', match[2]))))
            continue
        if line.startswith('bb.'):
            match = re.fullmatch(r'bb\.(\d+)(?: \([^\n]+\))?:', line)
            if not match:
                raise ValueError("unsupported machine block header")
            block = {"instructions": [], "successors": None}
            insert_unique(blocks, int(match[1]), block)
            continue
        if block is None:
            raise ValueError(f"unexpected machine metadata: {line}")
        if line.startswith('  liveins: '):
            if not re.fullmatch(r'  liveins: \$x\d+(?:, \$x\d+)*', line):
                raise ValueError("unsupported block liveins")
            continue
        if line.startswith('  successors: '):
            if block['successors'] is not None or not re.fullmatch(r'  successors: %bb\.\d+\(0x[0-9a-f]+\)(?:, %bb\.\d+\(0x[0-9a-f]+\))*(?:;.*)?', line):
                raise ValueError("malformed/duplicate machine successors")
            block['successors'] = set(map(int, re.findall(r'%bb\.(\d+)', line.split(';')[0])))
            continue
        text = line.strip().split(', debug-location')[0].split(' :: ')[0].split(';')[0]
        if not line.startswith('  ') or line.startswith('    '):
            raise ValueError("unparsed machine instruction")
        match = re.fullmatch(r'(?:(.*?) = )?(' + MACHINE_FLAGS + r')(\w+)(?: (.*))?', text)
        if not match or match[3] not in MACHINE_OPS:
            raise ValueError(f"unsupported machine instruction: {text}")
        destination, flags, opcode, operands = match.groups()
        operands = operands or ''
        if opcode.startswith('DBG_'):
            continue
        if destination is not None and not re.fullmatch(r'(?:renamable )?\$x(?:[0-9]|[12][0-9]|3[01])', destination):
            raise ValueError("unsupported machine register definition")
        delta = 0
        if destination == '$x2':
            adjustment = re.fullmatch(r'\$x2, (-?\d+)', operands)
            if opcode != 'ADDI' or not adjustment:
                raise ValueError("unproved machine SP adjustment")
            amount = int(adjustment[1])
            if not -2048 <= amount <= 2047 or amount % 16 or not amount:
                raise ValueError("unsupported machine SP immediate")
            if flags != ('frame-setup ' if amount < 0 else 'frame-destroy '):
                raise ValueError("SP write lacks ordinary frame setup/destroy provenance")
            delta = -amount
            sp_writes[amount] += 1
        elif destination is not None and re.search(r'\$x2\b', destination):
            raise ValueError("aliased SP write")
        if re.search(r'(?:implicit-def|\bdef\b)[^,]*\$(?:x2|sp)(?:\b|_)', operands):
            if opcode not in MACHINE_CALLS or re.findall(r'implicit-def[^,]*\$x2\b', operands) != ['implicit-def $x2']:
                raise ValueError("unsupported implicit SP definition")
        if opcode == 'INLINEASM':
            asm[benign_assembly(text)] += 1
        if opcode in {'PseudoCALL', 'PseudoTAIL'}:
            callee = re.match(r'target-flags\(riscv-call\) [@&]([^ ,]+), <regmask ', operands)
            if not callee or re.search(r'(?:__riscv_(?:save|restore)|morestack|longjmp|setjmp|swapcontext)', callee[1]):
                raise ValueError(f"nonstandard call/tail helper: {operands[:220]}")
            calls.add(callee[1])
        if opcode in {'PseudoBRIND', 'PseudoTAILIndirect'}:
            target = re.match(r'(?:killed )?(?:renamable )?\$x(\d+)(?:, |$)', operands)
            if not target or int(target[1]) in {1, 2}:
                raise ValueError("unsupported indirect transfer register")
            indirect.append((opcode, REGISTERS[int(target[1])]))
        if opcode == 'PseudoCALLIndirect' and '<regmask ' not in operands:
            raise ValueError("non-ABI indirect call")
        targets = set(map(int, re.findall(r'%bb\.(\d+)', operands))) if opcode in MACHINE_BRANCHES else set()
        block['instructions'].append((opcode, delta, targets))
    if not blocks or next(iter(blocks)) != 0:
        raise ValueError("missing machine entry block")
    table_targets = set().union(*tables.values()) if tables else set()
    if not table_targets <= blocks.keys():
        raise ValueError("missing jump-table block")
    for index, (number, block) in enumerate(blocks.items()):
        ins = block['instructions']
        if not ins:
            raise ValueError(f"empty machine block: {number}, successors={block['successors']}")
        targets = set().union(*(item[2] for item in ins))
        successors = block['successors'] or set()
        if not successors <= blocks.keys():
            raise ValueError("missing successor block")
        if not targets <= successors:
            raise ValueError("explicit branch target is absent from compiler successors")
        if ins[-1][0] == 'PseudoBRIND':
            if not successors or not successors <= table_targets:
                raise ValueError("local dispatch lacks compiler jump-table provenance")
        elif ins[-1][0] in MACHINE_EXITS:
            if successors:
                raise ValueError("nonlocal exit has local successors")
        elif ins[-1][0] in MACHINE_CALLS and not successors:
            if targets:
                raise ValueError("nonreturning call block has branch targets")
        else:
            if ins[-1][0] != 'PseudoBR' and index + 1 < len(blocks):
                targets.add(list(blocks)[index + 1])
            if targets != successors:
                raise ValueError(f"inconsistent compiler control flow: bb{number}, final={ins[-1][0]}, targets={targets}, successors={successors}")
    depths, pending, maximum = {0: 0}, deque([0]), 0
    while pending:
        number = pending.popleft()
        depth = depths[number]
        block = blocks[number]
        for opcode, delta, _ in block['instructions']:
            depth += delta
            if not 0 <= depth <= record['size']:
                raise ValueError("machine depth contradicts fixed frame")
            maximum = max(maximum, depth)
            if opcode in MACHINE_EXITS - {'UNIMP'} and depth:
                raise ValueError("non-ABI-balanced return/tail transfer")
        for child in block['successors'] or ():
            if child in depths and depths[child] != depth:
                raise ValueError("allocating/inconsistent machine stack-depth cycle or join")
            if child not in depths:
                depths[child] = depth
                pending.append(child)
    if set(depths) != set(blocks) or maximum != record['size']:
        raise ValueError("unreachable machine blocks or contradicting frame size")
    return {"size": maximum, "sp_writes": sp_writes, "indirect": indirect, "calls": sorted(calls),
            "blocks": len(blocks), "assembly": dict(asm)}


def corroborate(stack, body, proof, transfers=None):
    parsed = stack.instructions(body)
    stack.frame_cfg(parsed, transfers)
    writes, indirect = Counter(), []
    for address, opcode, args, target in parsed:
        if opcode not in stack.NO_DESTINATION and args[0] in {'sp', 'x2'}:
            if opcode != 'addi' or args[1] not in {'sp', 'x2'}:
                raise ValueError(f"unproved emitted SP adjustment at {address:x}")
            writes[int(args[2], 0)] += 1
        if opcode in {'jr', 'jalr'} and not stack.is_call(opcode, args) and args != ['ra'] and target is None:
            if len(args) != 1 or args[0] not in REGISTERS:
                raise ValueError("unsupported emitted indirect transfer")
            indirect.append((address, args[0]))
    if writes != proof['sp_writes']:
        raise ValueError(f"emitted SP writes contradict compiler provenance: {writes} != {proof['sp_writes']}")
    if [reg for _, reg in indirect] != [reg for _, reg in proof['indirect']]:
        raise ValueError("ambiguous emitted local/ABI-tail provenance")
    try:
        measured = stack.stack_frame(body, transfers)
    except ValueError as error:
        if not str(error).startswith(('unreachable stack write has no frame proof', 'unresolved in-frame transfer')):
            raise
    else:
        if measured != proof['size']:
            raise ValueError("successful disassembly frame contradicts compiler size")
    return {address for (address, _), (kind, _) in zip(indirect, proof['indirect']) if kind == 'PseudoBRIND'}


def symbol_inventory(stack, elf, functions, nm):
    import hashlib
    import struct

    stack.readonly_sections(elf)
    section_offset = struct.unpack_from('<I', elf, 32)[0]
    section_size, section_count = struct.unpack_from('<HH', elf, 46)
    code = []
    for index in range(section_count):
        _, kind, flags, base, offset, length, *_ = struct.unpack_from('<10I', elf, section_offset + index * section_size)
        if kind == 1 and flags & 6 == 6:
            if offset + length > len(elf):
                raise ValueError('truncated executable section')
            code.append((base, elf[offset:offset + length]))
    symbols = {}
    for line in nm.splitlines():
        match = re.fullmatch(r'([0-9a-f]+) ([0-9a-f]+) [tT] (\S+)', line)
        if match:
            address, size, raw = match.groups()
            symbols.setdefault(int(address, 16), []).append((int(size, 16), raw))
    poll = stack.reader_symbols(functions)
    result = {}
    for name, body in functions.items():
        if not name.lstrip('<').startswith(stack.SCOPES) and name != poll:
            continue
        parsed = stack.instructions(body)
        address = parsed[0][0]
        candidates = symbols.get(address, [])
        if len(candidates) != 1 or not candidates[0][0]:
            raise ValueError(f'missing/ambiguous raw symbol extent: {name}')
        size, raw = candidates[0]
        sections = [(base, data) for base, data in code if base <= address and address + size <= base + len(data)]
        if len(sections) != 1:
            raise ValueError('function lacks a unique complete executable extent')
        base, data = sections[0]
        function = data[address - base:address - base + size]
        offset = 0
        addresses = []
        while offset < size:
            if offset + 2 > size:
                raise ValueError('truncated emitted instruction')
            halfword = int.from_bytes(function[offset:offset + 2], 'little')
            if halfword & 31 == 31:
                raise ValueError('unsupported greater-than-32-bit instruction')
            addresses.append(address + offset)
            offset += 4 if halfword & 3 == 3 else 2
        if offset != size or addresses != [item[0] for item in parsed]:
            raise ValueError(f'disassembly does not decode every byte of selected extent: {name}; '
                             f'expected {len(addresses)} instructions, decoded {len(parsed)}')
        insert_unique(result, raw, {"name": name, "address": address, "size": size,
                                   "code_sha256": hashlib.sha256(function).hexdigest()})
    return result
