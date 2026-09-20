use crate::{re, stack};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap as Map, BTreeSet as Set, VecDeque};

pub const PROPERTIES: &str =
    "NoPHIs, TracksLiveness, NoVRegs, TiedOpsRewritten, TracksDebugUserValues";
const REGISTERS: &[&str] = &[
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];
const OPS: &str = "ADDI LW SW COPY LBU LUI PseudoMovAddr BEQ SB SUB BNE ANDI SLLI OR ADD SH BLTU LHU SRLI PseudoRET BGEU AND SLTU PseudoTAILIndirect LH SLTIU XOR PseudoBRIND PseudoCALLIndirect ORI BLT MUL XORI LB PseudoTAIL SRL UNIMP DIVU BGE SRAI SLTI SLL MULHU PseudoBR PseudoCALL INLINEASM IMPLICIT_DEF KILL MEMBARRIER CFI_INSTRUCTION DBG_VALUE DBG_VALUE_LIST";
const BRANCHES: &[&str] = &["BEQ", "BNE", "BLT", "BGE", "BLTU", "BGEU", "PseudoBR"];
const CALLS: &[&str] = &["PseudoCALL", "PseudoCALLIndirect"];
const EXITS: &[&str] = &["PseudoRET", "PseudoTAIL", "PseudoTAILIndirect", "UNIMP"];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub size: u64,
    #[serde(default)]
    pub slots: Vec<(i64, String, u64, u64)>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Proof {
    pub size: u64,
    #[serde(default)]
    pub sp_writes: Map<i64, usize>,
    #[serde(default)]
    pub indirect: Vec<(String, String)>,
    #[serde(default)]
    pub calls: Vec<String>,
    #[serde(default)]
    pub blocks: usize,
    #[serde(default)]
    pub assembly: Map<String, usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callees: Vec<String>,
}
fn valid_lines(lines: &[String]) -> Result<()> {
    ensure!(
        lines
            .iter()
            .all(|s| s.len() <= 1_000_000 && !s.contains('\x1b')),
        "oversized or decorated compiler record"
    );
    Ok(())
}
fn unique<K: Ord, V>(map: &mut Map<K, V>, key: K, value: V) -> Result<()> {
    ensure!(!map.contains_key(&key), "duplicate compiler record");
    map.insert(key, value);
    Ok(())
}
pub fn frame_records(source: &[String], selected: &Set<String>) -> Result<Map<String, Frame>> {
    valid_lines(source)?;
    let mut frames = Map::new();
    let mut layouts: Map<String, Vec<(i64, String, u64, u64)>> = Map::new();
    let mut layout: Option<String> = None;
    let mut awaiting = false;
    for line in source {
        let line = line.trim_end_matches('\n');
        let stripped = line.trim();
        if line.contains("prologepilog (analysis)") {
            let m = re(
                r"^note: .* prologepilog \(analysis\): (\d+) stack bytes in function '([^']+)'$",
            )
            .captures(stripped)
            .ok_or_else(|| anyhow::anyhow!("malformed fixed-frame record"))?;
            unique(&mut frames, m[2].to_owned(), m[1].parse::<u64>()?)?;
        } else if line.contains("stack-frame-layout (analysis)") {
            ensure!(
                re(r"^note: .* stack-frame-layout \(analysis\):\s*$").is_match(stripped)
                    && !awaiting,
                "malformed layout record"
            );
            awaiting = true;
            layout = None;
        } else if awaiting {
            let m = re(r"^Function: ([^ ]+)$")
                .captures(stripped)
                .ok_or_else(|| anyhow::anyhow!("missing layout function"))?;
            layout = Some(m[1].into());
            awaiting = false;
            unique(&mut layouts, m[1].to_owned(), vec![])?;
        } else if let Some(name) = layout.clone() {
            if stripped.is_empty() {
                layout = None;
            } else if line.starts_with("    ") && !line.contains("Offset:") {
                ensure!(
                    re(r"^\s+[^\n]+ @ [^\n]+$").is_match(line),
                    "malformed layout source location"
                );
            } else {
                let m=re(r"^Offset: \[SP([+-]\d+)\], Type: (Spill|Variable|Fixed), Align: (\d+), Size: (\d+)$").captures(stripped).ok_or_else(||anyhow::anyhow!("unsupported frame slot: {stripped}"))?;
                layouts.get_mut(&name).unwrap().push((
                    m[1].parse()?,
                    m[2].into(),
                    m[3].parse()?,
                    m[4].parse()?,
                ));
            }
        }
    }
    ensure!(!awaiting, "truncated layout record");
    let mut result = Map::new();
    for raw in selected {
        let size = *frames
            .get(raw)
            .ok_or_else(|| anyhow::anyhow!("missing explicit frame/layout record: {raw}"))?;
        let slots = layouts
            .remove(raw)
            .ok_or_else(|| anyhow::anyhow!("missing explicit frame/layout record: {raw}"))?;
        ensure!(
            size % 16 == 0 && size < 1 << 31,
            "unsupported fixed scalar frame size"
        );
        for (offset, _, align, len) in &slots {
            ensure!(
                [1, 2, 4, 8, 16].contains(align)
                    && *len <= size
                    && *offset >= -(size as i64)
                    && *offset <= -(*len as i64),
                "unsupported slot alignment/extent: {raw}"
            );
        }
        result.insert(raw.clone(), Frame { size, slots });
    }
    Ok(result)
}
pub fn machine_records(
    source: &[String],
    selected: &Set<String>,
) -> Result<Map<String, Vec<String>>> {
    valid_lines(source)?;
    let mut records = Map::new();
    let mut name: Option<String> = None;
    let mut body = vec![];
    let mut bytes = 0;
    let mut marker = false;
    for line in source {
        let line = line.trim_end_matches('\n');
        if line == "# *** IR Dump Before RISC-V Assembly Printer (riscv-asm-printer) ***:" {
            ensure!(name.is_none(), "interleaved machine records");
            marker = true;
        } else if line.starts_with("# Machine code for function ") {
            let m = re(r"^# Machine code for function ([^: ]+): (.+)$")
                .captures(line)
                .ok_or_else(|| anyhow::anyhow!("malformed/unbound machine header"))?;
            ensure!(name.is_none() && marker, "malformed/unbound machine header");
            ensure!(
                !selected.contains(&m[1]) || &m[2] == PROPERTIES,
                "unsupported machine function properties"
            );
            name = Some(m[1].into());
            body.clear();
            bytes = 0;
            marker = false;
        } else if line.starts_with("# End machine code for function ") {
            let m = re(r"^# End machine code for function ([^ ]+)\.$")
                .captures(line)
                .ok_or_else(|| anyhow::anyhow!("malformed/mismatched machine end"))?;
            ensure!(
                name.as_deref() == Some(&m[1]),
                "malformed/mismatched machine end"
            );
            if selected.contains(&m[1]) {
                unique(&mut records, m[1].to_owned(), body.clone())?;
            }
            name = None;
        } else if name.as_ref().is_some_and(|n| selected.contains(n)) {
            bytes += line.len();
            ensure!(
                bytes <= 16_000_000,
                "selected machine record exceeds parser bound"
            );
            body.push(line.into());
        }
    }
    ensure!(
        name.is_none() && records.keys().cloned().collect::<Set<_>>() == *selected,
        "missing/truncated selected machine record"
    );
    Ok(records)
}
pub fn benign_assembly(text: &str) -> Result<String> {
    let m = re(r#"^INLINEASM &"([^"\n]*)" (.+)$"#)
        .captures(text)
        .ok_or_else(|| anyhow::anyhow!("opaque inline assembly is not a fixed-frame proof"))?;
    let template = &m[1];
    let operands = &m[2];
    let role = match template {
        "csrrci ${0}, mstatus, 0x8" | "csrrci ${0}, mstatus, 8" => "regdef-ec",
        "csrrs x0, 0x300, ${0}" | "csrs mstatus, ${0}" => "reguse",
        _ => bail!("opaque inline assembly is not a fixed-frame proof"),
    };
    let operand = re(r"\$0:\[([^]]+)\], ([^,]+)")
        .captures(operands)
        .ok_or_else(|| anyhow::anyhow!("unsupported inline assembly operands"))?;
    ensure!(
        operand[1] == format!("{role}:GPRNoX0"),
        "unsupported inline assembly operands"
    );
    for m in re(r"\$([A-Za-z_][A-Za-z_0-9]*|\d+)").captures_iter(operands) {
        ensure!(
            ["vl", "vtype"].contains(&&m[1])
                || m[1].chars().all(|c| c.is_ascii_digit())
                || re(r"^x(?:[0-9]|[12][0-9]|3[01])$").is_match(&m[1]),
            "unknown inline assembly register or alias"
        );
    }
    let registers: Vec<_> = re(r"\$x(\d+)\b")
        .captures_iter(operands)
        .map(|m| m[1].parse::<usize>().unwrap())
        .collect();
    ensure!(
        !registers.is_empty()
            && registers
                .iter()
                .all(|n| (1..=31).contains(n) && ![1, 2, 3, 4, 8].contains(n)),
        "reserved register in inline assembly"
    );
    ensure!(
        !re(r"\$(?:sp|ra|fp|x2_\w+|x8_\w+)\b").is_match(operands),
        "stack/register alias in inline assembly"
    );
    for m in re(r"\$([1-9]\d*):\[([^]]*)\]").captures_iter(operands) {
        ensure!(
            ["clobber", "reguse tiedto:$0"].contains(&&m[2]),
            "unexpected inline assembly operand"
        );
    }
    for m in re(r"\[clobber\], implicit-def early-clobber (\$\w+)").captures_iter(operands) {
        ensure!(
            ["$vtype", "$vl"].contains(&&m[1]),
            "unsupported inline assembly clobber"
        );
    }
    Ok(template.into())
}
struct MachineInstruction {
    opcode: String,
    delta: i64,
    targets: Set<u32>,
}
struct Block {
    instructions: Vec<MachineInstruction>,
    successors: Option<Set<u32>>,
}
fn block_targets(text: &str) -> Set<u32> {
    re(r"%bb\.(\d+)")
        .captures_iter(text)
        .map(|m| m[1].parse().unwrap())
        .collect()
}
pub fn machine_frame(body: &[String], record: &Frame) -> Result<Proof> {
    let mut blocks: Map<u32, Block> = Map::new();
    let mut order = vec![];
    let mut tables: Map<String, Set<u32>> = Map::new();
    let mut object_ids = Set::new();
    let mut current = None;
    let mut proof = Proof {
        size: record.size,
        ..Proof::default()
    };
    let mut calls = Set::new();
    for line in body {
        if line.is_empty()
            || line.starts_with(';')
            || [
                "Frame Objects:",
                "save/restore points:",
                "save points are empty",
                "restore points are empty",
                "Jump Tables:",
            ]
            .contains(&line.as_str())
            || re(r"^Function Live Ins: (?:\$x\d+(?: in %\d+)?(?:, )?)+$").is_match(line)
        {
            continue;
        }
        if line.starts_with("  fi#") {
            let m = re(
                r"^  fi#(-?\d+): (?:dead|size=(\d+), align=(\d+), at location \[SP([+-]\d+)\])$",
            )
            .captures(line)
            .ok_or_else(|| anyhow::anyhow!("unsupported/duplicate machine frame object"))?;
            ensure!(
                object_ids.insert(m[1].to_owned()),
                "unsupported/duplicate machine frame object"
            );
            if let Some(len) = m.get(2) {
                let len = len.as_str().parse::<i64>()?;
                let align = m[3].parse::<u64>()?;
                let offset = m[4].parse::<i64>()?;
                ensure!(
                    [1, 2, 4, 8, 16].contains(&align)
                        && offset >= -(record.size as i64)
                        && offset <= -len,
                    "unsupported machine object alignment/extent"
                );
            }
            continue;
        }
        if line.starts_with("%jump-table.") {
            let m = re(r"^%jump-table\.(\d+): ((?:%bb\.\d+ ?)+)$")
                .captures(line)
                .ok_or_else(|| anyhow::anyhow!("malformed machine jump table"))?;
            unique(&mut tables, m[1].to_owned(), block_targets(&m[2]))?;
            continue;
        }
        if line.starts_with("bb.") {
            let m = re(r"^bb\.(\d+)(?: \([^\n]+\))?:$")
                .captures(line)
                .ok_or_else(|| anyhow::anyhow!("unsupported machine block header"))?;
            let id = m[1].parse::<u32>()?;
            unique(
                &mut blocks,
                id,
                Block {
                    instructions: vec![],
                    successors: None,
                },
            )?;
            order.push(id);
            current = Some(id);
            continue;
        }
        let block = blocks
            .get_mut(
                &current.ok_or_else(|| anyhow::anyhow!("unexpected machine metadata: {line}"))?,
            )
            .unwrap();
        if line.starts_with("  liveins: ") {
            ensure!(
                re(r"^  liveins: \$x\d+(?:, \$x\d+)*$").is_match(line),
                "unsupported block liveins"
            );
            continue;
        }
        if line.starts_with("  successors: ") {
            ensure!(block.successors.is_none()&&re(r"^  successors: %bb\.\d+\(0x[0-9a-f]+\)(?:, %bb\.\d+\(0x[0-9a-f]+\))*(?:;.*)?$").is_match(line),"malformed/duplicate machine successors");
            block.successors = Some(block_targets(line.split(';').next().unwrap()));
            continue;
        }
        let text = line
            .trim()
            .split(", debug-location")
            .next()
            .unwrap()
            .split(" :: ")
            .next()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        ensure!(
            line.starts_with("  ") && !line.starts_with("    "),
            "unparsed machine instruction"
        );
        let m=re(r"^(?:(.*?) = )?((?:(?:frame-setup|frame-destroy|nuw|nsw|disjoint|exact|samesign) )*)(\w+)(?: (.*))?$").captures(text).ok_or_else(||anyhow::anyhow!("unsupported machine instruction: {text}"))?;
        let destination = m.get(1).map(|v| v.as_str());
        let flags = &m[2];
        let opcode = &m[3];
        let operands = m.get(4).map_or("", |v| v.as_str());
        ensure!(
            OPS.split_whitespace().any(|v| v == opcode),
            "unsupported machine instruction: {text}"
        );
        if opcode.starts_with("DBG_") {
            continue;
        }
        ensure!(
            destination
                .is_none_or(|d| re(r"^(?:renamable )?\$x(?:[0-9]|[12][0-9]|3[01])$").is_match(d)),
            "unsupported machine register definition"
        );
        let mut delta = 0;
        if destination == Some("$x2") {
            let adjustment = re(r"^\$x2, (-?\d+)$")
                .captures(operands)
                .ok_or_else(|| anyhow::anyhow!("unproved machine SP adjustment"))?;
            ensure!(opcode == "ADDI", "unproved machine SP adjustment");
            let amount = adjustment[1].parse::<i64>()?;
            ensure!(
                (-2048..=2047).contains(&amount) && amount % 16 == 0 && amount != 0,
                "unsupported machine SP immediate"
            );
            ensure!(
                flags
                    == if amount < 0 {
                        "frame-setup "
                    } else {
                        "frame-destroy "
                    },
                "SP write lacks ordinary frame setup/destroy provenance"
            );
            delta = -amount;
            *proof.sp_writes.entry(amount).or_default() += 1;
        } else if destination.is_some_and(|d| re(r"\$x2\b").is_match(d)) {
            bail!("aliased SP write")
        }
        if re(r"(?:implicit-def|\bdef\b)[^,]*\$(?:x2|sp)(?:\b|_)").is_match(operands) {
            let defs: Vec<_> = re(r"implicit-def[^,]*\$x2\b")
                .find_iter(operands)
                .map(|m| m.as_str())
                .collect();
            ensure!(
                CALLS.contains(&opcode) && defs == ["implicit-def $x2"],
                "unsupported implicit SP definition"
            );
        }
        if opcode == "INLINEASM" {
            *proof.assembly.entry(benign_assembly(text)?).or_default() += 1;
        }
        if ["PseudoCALL", "PseudoTAIL"].contains(&opcode) {
            let callee = re(r"^target-flags\(riscv-call\) [@&]([^ ,]+), <regmask ")
                .captures(operands)
                .ok_or_else(|| anyhow::anyhow!("nonstandard call/tail helper"))?;
            ensure!(
                !re(r"(?:__riscv_(?:save|restore)|morestack|longjmp|setjmp|swapcontext)")
                    .is_match(&callee[1]),
                "nonstandard call/tail helper"
            );
            calls.insert(callee[1].to_owned());
        }
        if ["PseudoBRIND", "PseudoTAILIndirect"].contains(&opcode) {
            let target = re(r"^(?:killed )?(?:renamable )?\$x(\d+)(?:, |$)")
                .captures(operands)
                .ok_or_else(|| anyhow::anyhow!("unsupported indirect transfer register"))?;
            let reg = target[1].parse::<usize>()?;
            ensure!(
                reg < 32 && ![1, 2].contains(&reg),
                "unsupported indirect transfer register"
            );
            proof.indirect.push((opcode.into(), REGISTERS[reg].into()));
        }
        ensure!(
            opcode != "PseudoCALLIndirect" || operands.contains("<regmask "),
            "non-ABI indirect call"
        );
        block.instructions.push(MachineInstruction {
            opcode: opcode.into(),
            delta,
            targets: if BRANCHES.contains(&opcode) {
                block_targets(operands)
            } else {
                Set::new()
            },
        });
    }
    ensure!(order.first() == Some(&0), "missing machine entry block");
    let table_targets: Set<_> = tables.values().flatten().copied().collect();
    let ids: Set<_> = blocks.keys().copied().collect();
    ensure!(table_targets.is_subset(&ids), "missing jump-table block");
    for (index, number) in order.iter().enumerate() {
        let block = &blocks[number];
        let last = block
            .instructions
            .last()
            .ok_or_else(|| anyhow::anyhow!("empty machine block: {number}"))?;
        let mut targets: Set<_> = block
            .instructions
            .iter()
            .flat_map(|i| i.targets.iter().copied())
            .collect();
        let successors = block.successors.clone().unwrap_or_default();
        ensure!(successors.is_subset(&ids), "missing successor block");
        ensure!(
            targets.is_subset(&successors),
            "explicit branch target is absent from compiler successors"
        );
        if last.opcode == "PseudoBRIND" {
            ensure!(
                !successors.is_empty() && successors.is_subset(&table_targets),
                "local dispatch lacks compiler jump-table provenance"
            );
        } else if EXITS.contains(&last.opcode.as_str()) {
            ensure!(successors.is_empty(), "nonlocal exit has local successors");
        } else if CALLS.contains(&last.opcode.as_str()) && successors.is_empty() {
            ensure!(
                targets.is_empty(),
                "nonreturning call block has branch targets"
            );
        } else {
            if last.opcode != "PseudoBR"
                && let Some(next) = order.get(index + 1)
            {
                targets.insert(*next);
            }
            ensure!(
                targets == successors,
                "inconsistent compiler control flow: bb{number}"
            );
        }
    }
    let mut depths = Map::from([(0, 0i64)]);
    let mut pending = VecDeque::from([0]);
    let mut maximum = 0;
    while let Some(number) = pending.pop_front() {
        let mut depth = depths[&number];
        let block = &blocks[&number];
        for i in &block.instructions {
            depth += i.delta;
            ensure!(
                depth >= 0 && depth <= record.size as i64,
                "machine depth contradicts fixed frame"
            );
            maximum = maximum.max(depth);
            ensure!(
                !EXITS.contains(&i.opcode.as_str()) || i.opcode == "UNIMP" || depth == 0,
                "non-ABI-balanced return/tail transfer"
            );
        }
        for child in block.successors.iter().flatten() {
            if let Some(old) = depths.get(child) {
                ensure!(
                    *old == depth,
                    "allocating/inconsistent machine stack-depth cycle or join"
                );
            } else {
                depths.insert(*child, depth);
                pending.push_back(*child);
            }
        }
    }
    ensure!(
        depths.len() == blocks.len() && maximum as u64 == record.size,
        "unreachable machine blocks or contradicting frame size"
    );
    proof.blocks = blocks.len();
    proof.calls = calls.into_iter().collect();
    Ok(proof)
}
pub fn corroborate(
    body: &[String],
    proof: &Proof,
    transfers: &stack::Transfers,
) -> Result<Set<u32>> {
    let parsed = stack::instructions(body)?;
    stack::frame_cfg(&parsed, transfers)?;
    let mut writes = Map::new();
    let mut indirect = vec![];
    for (address, op, args, target) in parsed {
        if !stack::NO_DESTINATION.contains(&op.as_str()) && ["sp", "x2"].contains(&args[0].as_str())
        {
            ensure!(
                op == "addi" && ["sp", "x2"].contains(&args[1].as_str()),
                "unproved emitted SP adjustment at {address:x}"
            );
            *writes.entry(crate::number(&args[2])?).or_insert(0usize) += 1;
        }
        if ["jr", "jalr"].contains(&op.as_str())
            && !stack::is_call(&op, &args)
            && args != ["ra"]
            && target.is_none()
        {
            ensure!(
                args.len() == 1 && REGISTERS.contains(&args[0].as_str()),
                "unsupported emitted indirect transfer"
            );
            indirect.push((address, args[0].clone()));
        }
    }
    ensure!(
        writes == proof.sp_writes,
        "emitted SP writes contradict compiler provenance"
    );
    ensure!(
        indirect
            .iter()
            .map(|(_, r)| r)
            .eq(proof.indirect.iter().map(|(_, r)| r)),
        "ambiguous emitted local/ABI-tail provenance"
    );
    match stack::stack_frame(body, transfers) {
        Ok(measured) => ensure!(
            measured == proof.size,
            "successful disassembly frame contradicts compiler size"
        ),
        Err(e) => {
            let text = e.to_string();
            if !text.starts_with("unreachable stack write has no frame proof")
                && !text.starts_with("unresolved in-frame transfer")
            {
                return Err(e);
            }
        }
    }
    Ok(indirect
        .into_iter()
        .zip(&proof.indirect)
        .filter(|(_, (kind, _))| kind == "PseudoBRIND")
        .map(|((a, _), _)| a)
        .collect())
}
