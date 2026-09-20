use crate::{number, re};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap as Map, BTreeSet as Set, VecDeque};

pub const TASK: &str = "brewthink::x4::reader_app::__reader_app_task_task::__reader_app_task_task_inner_function::{closure#0}";
pub const LIBRARY: &str = "brewthink::x4::reader_app::load_library";
pub const EFFECT: &str = "brewthink::x4::reader_app::run_effect";
pub const RESERVE: u64 = 8192;
pub const SCOPES: &[&str] = &[
    "brewthink::x4::reader_app::",
    "brewthink::reader_orchestration::",
    "brewthink::storage::book_resume::",
    "brewthink::device_epub::",
    "brewthink::zip_stream::",
    "brewthink::scratch::",
    "brewthink::bounded_layout::",
];
const BRANCHES: &[&str] = &[
    "beq", "bne", "blt", "bge", "bltu", "bgeu", "beqz", "bnez", "blez", "bgez", "bltz", "bgtz",
    "bgt", "ble", "bgtu", "bleu",
];
pub const NO_DESTINATION: &[&str] = &[
    "sw", "sh", "sb", "fsw", "fsd", "fence", "fence.i", "nop", "csrc", "csrs", "csrw", "csrci",
    "csrsi", "csrwi",
];
const DESTINATION: &[&str] = &[
    "add", "addi", "and", "andi", "auipc", "div", "divu", "lb", "lbu", "lh", "lhu", "li", "lui",
    "lw", "mul", "mulh", "mulhsu", "mulhu", "mv", "neg", "not", "or", "ori", "rem", "remu", "seqz",
    "sgtz", "sll", "slli", "slt", "slti", "sltiu", "sltu", "sltz", "snez", "sra", "srai", "srl",
    "srli", "sub", "xor", "xori", "zext.b", "zext.h", "sext.b", "sext.h", "csrr", "csrrc",
    "csrrci", "csrrs", "csrrsi", "csrrw", "csrrwi",
];
pub type Instruction = (u32, String, Vec<String>, Option<String>);
pub type Functions = Map<String, Vec<String>>;
pub type Transfers = Map<u32, Set<u32>>;
pub type Registers = Map<String, Set<u32>>;
pub type Sections = Vec<(u32, Vec<u8>)>;
pub type Edges = Map<String, Set<String>>;
#[derive(Debug, Serialize, Deserialize)]
pub struct TransferEvidence(pub Transfers, pub Transfers);

pub fn instructions(body: &[String]) -> Result<Vec<Instruction>> {
    let mut parsed = vec![];
    for line in body {
        if let Some(m) = re(r"^\s*([0-9a-f]+):\s+([a-z][a-z0-9.]*)\s*(.*)").captures(line) {
            let mut operands = m[3].to_owned();
            let target = re(r" <(.+)>$")
                .captures(&operands)
                .map(|a| (a.get(0).unwrap().start(), a[1].to_owned()));
            if let Some((start, _)) = &target {
                operands.truncate(*start);
            }
            parsed.push((
                u32::from_str_radix(&m[1], 16)?,
                m[2].into(),
                operands.split(',').map(|s| s.trim().into()).collect(),
                target.map(|(_, t)| t),
            ));
        } else {
            ensure!(
                !re(r"^\s*[0-9a-f]+:").is_match(line),
                "unparsed instruction: {line}"
            );
        }
    }
    ensure!(!parsed.is_empty(), "empty disassembly body");
    Ok(parsed)
}
pub fn control_flow(op: &str) -> bool {
    BRANCHES.contains(&op)
        || [
            "jal", "jalr", "j", "jr", "ret", "call", "tail", "unimp", "ebreak",
        ]
        .contains(&op)
}
pub fn is_call(op: &str, args: &[String]) -> bool {
    op == "call"
        || (["jal", "jalr"].contains(&op)
            && (args.len() == 1 || ["ra", "x1"].contains(&args[0].as_str())))
}
fn direct(args: &[String]) -> Option<u32> {
    args.iter()
        .find(|a| re(r"^0x[0-9a-f]+$").is_match(a))
        .and_then(|a| u32::from_str_radix(&a[2..], 16).ok())
}
pub fn frame_cfg(
    parsed: &[Instruction],
    transfers: &Transfers,
) -> Result<(Vec<Vec<usize>>, Set<usize>)> {
    let addresses: Map<_, _> = parsed.iter().enumerate().map(|(i, p)| (p.0, i)).collect();
    ensure!(
        addresses.len() == parsed.len(),
        "duplicate instruction address"
    );
    let targets: Set<_> = parsed
        .iter()
        .filter(|p| control_flow(&p.1))
        .filter_map(|p| direct(&p.2))
        .collect();
    let mut successors = vec![];
    let mut unresolved = Set::new();
    for (i, (address, op, args, _)) in parsed.iter().enumerate() {
        let following = if i + 1 < parsed.len() {
            vec![i + 1]
        } else {
            vec![]
        };
        ensure!(
            control_flow(op)
                || NO_DESTINATION.contains(&op.as_str())
                || DESTINATION.contains(&op.as_str()),
            "unsupported instruction at {address:x}: {op}"
        );
        ensure!(
            !["jal", "jalr"].contains(&op.as_str())
                || args.len() <= 1
                || ["ra", "x1", "zero", "x0"].contains(&args[0].as_str()),
            "unsupported link register at {address:x}"
        );
        if is_call(op, args) {
            successors.push(following);
        } else if ["ret", "unimp", "ebreak"].contains(&op.as_str())
            || (op == "jr" && args == &["ra"])
        {
            successors.push(vec![]);
        } else if control_flow(op) {
            let mut destination =
                if BRANCHES.contains(&op.as_str()) || ["j", "jal", "tail"].contains(&op.as_str()) {
                    direct(args)
                } else {
                    None
                };
            if destination.is_none()
                && i > 0
                && !targets.contains(address)
                && let Some(m) = re(r"^(-?0x[0-9a-f]+)\((\w+)\)$").captures(args.last().unwrap())
            {
                let (prev, pop, pa, _) = &parsed[i - 1];
                if pop == "auipc" && pa[0] == m[2] {
                    destination = Some(
                        (i64::from(*prev) + (number(&pa[1])? << 12) as i32 as i64 + number(&m[1])?)
                            as u32
                            & 0xfffffffe,
                    );
                }
            }
            let destinations = destination
                .map(|d| Set::from([d]))
                .or_else(|| transfers.get(address).cloned());
            let mut branches = vec![];
            if let Some(ds) = destinations {
                for d in ds {
                    if let Some(child) = addresses.get(&d) {
                        branches.push(*child);
                    } else {
                        ensure!(
                            d < parsed[0].0 || d > parsed.last().unwrap().0,
                            "branch into an unparsed instruction at {address:x}"
                        );
                    }
                }
            } else {
                ensure!(
                    !BRANCHES.contains(&op.as_str()) && op != "j",
                    "unknown branch target at {address:x}"
                );
                unresolved.insert(i);
            }
            if BRANCHES.contains(&op.as_str()) {
                branches.extend(following);
            }
            successors.push(branches);
        } else {
            successors.push(following);
        }
    }
    Ok((successors, unresolved))
}
fn constants(op: &str, args: &[String], regs: &Map<String, i64>) -> Result<Map<String, i64>> {
    if is_call(op, args) {
        return Ok(Map::from([("zero".into(), 0)]));
    }
    if control_flow(op) || NO_DESTINATION.contains(&op) {
        return Ok(regs.clone());
    }
    let mut next = regs.clone();
    let value = match op {
        "li" => Some(number(&args[1])?),
        "lui" => Some(number(&args[1])? << 12),
        "addi" | "add" | "sub" | "mv" => {
            let left = regs.get(&args[1]).copied();
            let right = match op {
                "mv" => Some(0),
                "addi" => Some(number(&args[2])?),
                _ => regs.get(&args[2]).copied(),
            };
            left.zip(right)
                .map(|(a, b)| if op == "sub" { a - b } else { a + b })
        }
        _ => None,
    };
    if !["zero", "x0", "sp", "x2"].contains(&args[0].as_str()) {
        if let Some(v) = value {
            next.insert(args[0].clone(), v as i32 as i64);
        } else {
            next.remove(&args[0]);
        }
    }
    Ok(next)
}
pub fn finite(values: impl IntoIterator<Item = i64>) -> Option<Set<u32>> {
    let mut result = Set::new();
    for v in values {
        result.insert(v as u32);
        if result.len() > 1024 {
            return None;
        }
    }
    Some(result)
}
fn load(addresses: Option<Set<u32>>, width: usize, sections: &Sections) -> Option<Set<u32>> {
    let mut values = vec![];
    for address in addresses? {
        let (base, data) = sections.iter().find(|(b, d)| {
            *b <= address && u64::from(address) + width as u64 <= u64::from(*b) + d.len() as u64
        })?;
        if !(address as usize).is_multiple_of(width) {
            return None;
        }
        let start = (address - base) as usize;
        let mut word = [0; 4];
        word[..width].copy_from_slice(&data[start..start + width]);
        values.push(i64::from(u32::from_le_bytes(word)));
    }
    finite(values)
}
fn register_values(
    address: u32,
    op: &str,
    args: &[String],
    regs: &Registers,
    sections: &Sections,
) -> Result<Registers> {
    if is_call(op, args) {
        return Ok(regs
            .iter()
            .filter(|(n, _)| n.as_str() == "zero" || re(r"^s(?:[0-9]|1[01])$").is_match(n))
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect());
    }
    if control_flow(op) || NO_DESTINATION.contains(&op) {
        return Ok(regs.clone());
    }
    let mut result = regs.clone();
    let mut value = None;
    match op {
        "li" => value = finite([number(&args[1])?]),
        "lui" | "auipc" => {
            value = finite([
                (number(&args[1])? << 12) + if op == "auipc" { i64::from(address) } else { 0 }
            ])
        }
        "lw" | "lbu" | "lb" | "lhu" | "lh" => {
            if let Some(m) = re(r"^(-?0x[0-9a-f]+|[0-9]+)\((\w+)\)$").captures(&args[1]) {
                let offset = number(&m[1])?;
                let addresses = regs
                    .get(&m[2])
                    .and_then(|s| finite(s.iter().map(|v| i64::from(*v) + offset)));
                let width = match op {
                    "lw" => 4,
                    "lb" | "lbu" => 1,
                    _ => 2,
                };
                value = load(addresses, width, sections);
                if ["lb", "lh"].contains(&op) {
                    let sign = 1u32 << (width * 8 - 1);
                    value = value.and_then(|v| {
                        finite(v.into_iter().map(|n| i64::from(n ^ sign) - i64::from(sign)))
                    });
                }
            }
            if value.is_none() && ["lbu", "lb"].contains(&op) {
                value = finite(if op == "lbu" { 0..256 } else { -128..128 });
            }
        }
        "sltiu" | "sltu" | "slti" | "slt" | "seqz" | "snez" => value = Some(Set::from([0, 1])),
        "add" | "addi" | "sub" | "mv" | "and" | "andi" | "slli" | "srli" | "zext.b" => {
            let left = regs.get(&args[1]);
            if op == "zext.b" {
                value = left.map_or_else(
                    || finite(0..256),
                    |s| finite(s.iter().map(|v| i64::from(v & 255))),
                );
            } else if op == "mv" {
                value = left.cloned();
            } else {
                let immediate = if ["addi", "andi", "slli", "srli"].contains(&op) {
                    finite([number(&args[2])?])
                } else {
                    regs.get(&args[2]).cloned()
                };
                if let (Some(a), Some(b)) = (left, immediate) {
                    value = finite(a.iter().flat_map(|a| {
                        b.iter().map(move |b| {
                            i64::from(match op {
                                "add" | "addi" => a.wrapping_add(*b),
                                "sub" => a.wrapping_sub(*b),
                                "and" | "andi" => a & b,
                                "slli" => a.wrapping_shl(b & 31),
                                "srli" => a >> (b & 31),
                                _ => unreachable!(),
                            })
                        })
                    }));
                } else if op == "andi" {
                    let n = number(&args[2])?;
                    if (0..1024).contains(&n) {
                        value = finite(0..=n);
                    }
                }
            }
        }
        _ => {}
    }
    if !["zero", "x0", "sp", "x2"].contains(&args[0].as_str()) {
        if let Some(v) = value {
            result.insert(args[0].clone(), v);
        } else {
            result.remove(&args[0]);
        }
    }
    Ok(result)
}
pub fn branch_values(
    op: &str,
    args: &[String],
    regs: &Registers,
    taken: bool,
) -> Option<Registers> {
    let mut result = regs.clone();
    if !["bltu", "bgeu"].contains(&op) {
        return Some(result);
    }
    let less = taken == (op == "bltu");
    let (left, right) = if less {
        (&args[0], &args[1])
    } else {
        (&args[1], &args[0])
    };
    let inclusive = !less;
    let mut lhs = result.get(left).cloned();
    let rhs = result.get(right).cloned();
    if let Some(ref r) = rhs {
        let upper = r
            .last()
            .map(|v| u64::from(*v) + u64::from(inclusive))
            .unwrap_or(0);
        if lhs.is_none() && upper <= 1024 {
            lhs = Some((0..upper as u32).collect());
        }
        if let Some(ref l) = lhs {
            result.insert(
                left.clone(),
                l.iter()
                    .copied()
                    .filter(|v| u64::from(*v) < upper)
                    .collect(),
            );
        }
    }
    if let Some(ref l) = lhs
        && let (Some(min), Some(r)) = (l.first(), rhs)
    {
        let lower = i64::from(*min) - i64::from(inclusive);
        result.insert(
            right.clone(),
            r.into_iter().filter(|v| i64::from(*v) > lower).collect(),
        );
    }
    if result.values().any(Set::is_empty) {
        None
    } else {
        Some(result)
    }
}
pub fn resolve_transfers(parsed: &[Instruction], sections: &Sections) -> Result<TransferEvidence> {
    let mut transfers = Transfers::new();
    loop {
        let (successors, unresolved) = frame_cfg(parsed, &transfers)?;
        let mut envs = Map::from([(0, Map::from([("zero".into(), Set::from([0]))]))]);
        let mut pending = VecDeque::from([0]);
        let mut queued = Set::from([0]);
        while let Some(i) = pending.pop_front() {
            queued.remove(&i);
            let (address, op, args, _) = &parsed[i];
            let outgoing = register_values(*address, op, args, &envs[&i], sections)?;
            for child in &successors[i] {
                let values = if successors[i].iter().filter(|v| *v == child).count() > 1 {
                    Some(outgoing.clone())
                } else {
                    branch_values(op, args, &outgoing, *child != i + 1)
                };
                let Some(values) = values else { continue };
                let joined = if let Some(previous) = envs.get(child) {
                    previous
                        .iter()
                        .filter_map(|(key, old)| {
                            values
                                .get(key)
                                .and_then(|new| finite(old.union(new).map(|v| i64::from(*v))))
                                .map(|v| (key.clone(), v))
                        })
                        .collect()
                } else {
                    values
                };
                if envs.get(child) != Some(&joined) {
                    envs.insert(*child, joined);
                    if queued.insert(*child) {
                        pending.push_back(*child);
                    }
                }
            }
        }
        let mut complete = Transfers::new();
        let mut changed = false;
        for (i, (address, _, args, _)) in parsed.iter().enumerate() {
            let Some(env) = envs.get(&i) else { continue };
            if !unresolved.contains(&i) && !transfers.contains_key(address) {
                continue;
            }
            let m = re(r"^(-?0x[0-9a-f]+|[0-9]+)\((\w+)\)$").captures(args.last().unwrap());
            let (register, offset) = if let Some(ref m) = m {
                (&m[2], number(&m[1])?)
            } else {
                (args.last().unwrap().as_str(), 0)
            };
            let Some(values) = env.get(register) else {
                continue;
            };
            let targets: Set<_> = values
                .iter()
                .map(|v| (i64::from(*v) + offset) as u32 & 0xfffffffe)
                .collect();
            complete.insert(*address, targets.clone());
            let joined: Set<_> = transfers
                .get(address)
                .into_iter()
                .flatten()
                .copied()
                .chain(targets)
                .collect();
            if transfers.get(address) != Some(&joined) {
                transfers.insert(*address, joined);
                changed = true;
            }
        }
        if !changed {
            return Ok(TransferEvidence(complete, transfers));
        }
    }
}
pub fn stack_frame(body: &[String], transfers: &Transfers) -> Result<u64> {
    let parsed = instructions(body)?;
    let (successors, unresolved) = frame_cfg(&parsed, transfers)?;
    let mut constants_at = Map::from([(0, Map::from([("zero".into(), 0)]))]);
    let mut pending = VecDeque::from([0]);
    while let Some(i) = pending.pop_front() {
        let (_, op, args, _) = &parsed[i];
        let outgoing = constants(op, args, &constants_at[&i])?;
        for child in &successors[i] {
            let joined = if let Some(previous) = constants_at.get(child) {
                previous
                    .iter()
                    .filter(|(key, value)| outgoing.get(*key) == Some(*value))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect()
            } else {
                outgoing.clone()
            };
            if constants_at.get(child) != Some(&joined) {
                constants_at.insert(*child, joined);
                pending.push_back(*child);
            }
        }
    }
    let mut deltas = vec![0i64; parsed.len()];
    for (i, (address, op, args, _)) in parsed.iter().enumerate() {
        if NO_DESTINATION.contains(&op.as_str()) || !["sp", "x2"].contains(&args[0].as_str()) {
            continue;
        }
        let regs = constants_at.get(&i).ok_or_else(|| {
            anyhow::anyhow!("unreachable stack write has no frame proof at {address:x}")
        })?;
        let delta = if ["addi", "add", "sub", "mv"].contains(&op.as_str()) && args[1] == "sp" {
            match op.as_str() {
                "mv" => Some(0),
                "addi" => Some(number(&args[2])?),
                _ => regs
                    .get(&args[2])
                    .map(|v| if op == "sub" { -*v } else { *v }),
            }
        } else {
            None
        };
        deltas[i] =
            -delta.ok_or_else(|| anyhow::anyhow!("unknown stack adjustment at {address:x}"))?;
    }
    let mut depths = Map::from([(0, (0i64, 0i64))]);
    let mut maximum = 0;
    let mut settled = false;
    for _ in 0..parsed.len() {
        let mut changed = false;
        for i in 0..parsed.len() {
            let Some((low, high)) = depths.get(&i).copied() else {
                continue;
            };
            let (low, high) = (low + deltas[i], high + deltas[i]);
            maximum = maximum.max(high);
            for child in &successors[i] {
                let joined = depths
                    .get(child)
                    .map_or((low, high), |(a, b)| ((*a).min(low), (*b).max(high)));
                if depths.get(child) != Some(&joined) {
                    depths.insert(*child, joined);
                    changed = true;
                }
            }
        }
        if !changed {
            settled = true;
            break;
        }
    }
    ensure!(settled, "unbounded or inconsistent stack-depth cycle");
    for (i, (low, high)) in depths {
        let address = parsed[i].0;
        ensure!(
            low + deltas[i] >= 0 && high + deltas[i] < 1 << 31,
            "stack depth outside supported entry-relative range at {address:x}"
        );
        ensure!(
            !unresolved.contains(&i) || (low, high) == (0, 0),
            "unresolved in-frame transfer at {address:x}; internal targets require proof"
        );
    }
    Ok(maximum as u64)
}
#[derive(Clone)]
pub struct Section {
    pub kind: u32,
    pub flags: u32,
    pub address: u32,
    pub offset: usize,
    pub size: usize,
    pub entry_size: usize,
}
fn word(elf: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        elf.get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("truncated ELF"))?
            .try_into()?,
    ))
}
fn half(elf: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        elf.get(at..at + 2)
            .ok_or_else(|| anyhow::anyhow!("truncated ELF"))?
            .try_into()?,
    ))
}
pub fn sections(elf: &[u8]) -> Result<Vec<Section>> {
    ensure!(
        elf.len() >= 52 && elf[..7] == *b"\x7fELF\x01\x01\x01" && half(elf, 18)? == 243,
        "expected a complete little-endian ELF32 RISC-V header"
    );
    let offset = word(elf, 32)? as usize;
    let size = half(elf, 46)? as usize;
    let count = half(elf, 48)? as usize;
    ensure!(
        size == 40 && count > 0 && offset + size * count <= elf.len(),
        "missing or truncated ELF section table"
    );
    (0..count)
        .map(|i| {
            let p = offset + i * size;
            Ok(Section {
                kind: word(elf, p + 4)?,
                flags: word(elf, p + 8)?,
                address: word(elf, p + 12)?,
                offset: word(elf, p + 16)? as usize,
                size: word(elf, p + 20)? as usize,
                entry_size: word(elf, p + 36)? as usize,
            })
        })
        .collect()
}
pub fn readonly_sections(elf: &[u8]) -> Result<Sections> {
    let mut result: Sections = vec![];
    for s in sections(elf)? {
        if s.kind != 1 || s.flags & 2 == 0 || s.flags & 5 != 0 || s.size == 0 {
            continue;
        }
        ensure!(
            s.offset + s.size <= elf.len() && u64::from(s.address) + s.size as u64 <= 1 << 32,
            "truncated or overflowing readonly ELF section"
        );
        ensure!(
            !result.iter().any(|(base, data)| u64::from(s.address)
                < u64::from(*base) + data.len() as u64
                && u64::from(*base) < u64::from(s.address) + s.size as u64),
            "overlapping readonly ELF sections"
        );
        result.push((s.address, elf[s.offset..s.offset + s.size].to_vec()));
    }
    ensure!(!result.is_empty(), "ELF has no immutable allocated data");
    Ok(result)
}
pub fn function_extents(elf: &[u8]) -> Result<Map<u32, u32>> {
    readonly_sections(elf)?;
    let sections = sections(elf)?;
    let mut extents = Map::new();
    for s in &sections {
        if s.kind != 2 {
            continue;
        }
        ensure!(
            s.entry_size == 16 && s.size % 16 == 0 && s.offset + s.size <= elf.len(),
            "malformed ELF symbol table"
        );
        for p in (s.offset..s.offset + s.size).step_by(16) {
            let address = word(elf, p + 4)?;
            let size = word(elf, p + 8)?;
            let info = elf[p + 12];
            let index = half(elf, p + 14)? as usize;
            if info & 15 != 2 || index == 0 || index >= 0xff00 {
                continue;
            }
            let target = sections
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("function has an invalid ELF section"))?;
            ensure!(
                target.kind == 1
                    && target.flags & 6 == 6
                    && target.offset + target.size <= elf.len(),
                "function lacks complete executable storage"
            );
            let end = u64::from(address) + u64::from(size);
            let section_end = u64::from(target.address) + target.size as u64;
            ensure!(
                target.address <= address && end <= section_end && section_end <= 1 << 32,
                "function exceeds its executable section"
            );
            let old = *extents.get(&address).unwrap_or(&size);
            ensure!(
                old == 0 || size == 0 || old == size,
                "conflicting function extents at one address"
            );
            extents.insert(address, old.max(size));
        }
    }
    ensure!(!extents.is_empty(), "ELF has no defined function symbols");
    Ok(extents)
}
pub fn functions_from(disassembly: &str, extents: &Map<u32, u32>) -> Result<Functions> {
    let mut functions = Functions::new();
    let mut owner: Option<String> = None;
    let mut addresses: Map<String, String> = Map::new();
    let mut owner_start = 0;
    let mut owner_end = None;
    for line in disassembly.lines() {
        if let Some(m) = re(r"^([0-9a-f]+) <(.+)>:$").captures(line) {
            let start = u32::from_str_radix(&m[1], 16)?;
            if owner_end.is_some_and(|end| owner_start < start && start < end)
                && !extents.contains_key(&start)
            {
                continue;
            }
            owner_start = start;
            owner_end = extents
                .get(&start)
                .map(|size| u64::from(start) + u64::from(*size))
                .and_then(|v| u32::try_from(v).ok());
            let mut name = m[2].to_owned();
            if let Some(previous) = addresses.get(&name) {
                if let Some(body) = functions.remove(&name) {
                    functions.insert(format!("{name} [0x{previous}]"), body);
                }
                name = format!("{name} [0x{}]", &m[1]);
            } else {
                addresses.insert(name.clone(), m[1].into());
            }
            ensure!(
                !functions.contains_key(&name),
                "duplicate symbol and address: {name}"
            );
            functions.insert(name.clone(), vec![]);
            owner = Some(name);
        } else if let Some(name) = &owner {
            functions.get_mut(name).unwrap().push(line.into());
        }
    }
    Ok(functions)
}
pub fn reader_symbols(functions: &Functions) -> Result<String> {
    let polls: Vec<_> = functions
        .keys()
        .filter(|n| n.contains(&format!("TaskStorage<{TASK}")) && n.ends_with("::poll"))
        .collect();
    ensure!(
        polls.len() == 1,
        "reader task poll symbol is missing or ambiguous"
    );
    for name in [LIBRARY, EFFECT] {
        ensure!(
            functions.contains_key(name),
            "required symbol missing; review coverage rather than assuming inlining: {name}"
        );
    }
    Ok(polls[0].clone())
}
pub fn check(frames: &Map<String, u64>, available: u64) -> Result<u64> {
    let required = frames["task"] + frames["library"].max(frames["effects"]) + RESERVE;
    ensure!(
        frames["task"] <= 4096 && required <= available,
        "reader stack budget exceeded: required={required}, available={available}"
    );
    Ok(required)
}

pub fn direct_graph(
    functions: &Functions,
    selected: &Set<String>,
    transfers: &Map<String, Transfers>,
    discovered: &Map<String, Transfers>,
) -> Result<(Edges, Vec<Value>)> {
    let mut edges: Edges = selected.iter().map(|s| (s.clone(), Set::new())).collect();
    let mut frontier = vec![];
    let mut entries = Map::new();
    for name in selected {
        entries.insert(instructions(&functions[name])?[0].0, name.clone());
    }
    for name in selected {
        let body = instructions(&functions[name])?;
        for (address, op, args, target) in &body {
            if !control_flow(op) || op == "ret" || (op == "jr" && args == &["ra"]) {
                continue;
            }
            if (BRANCHES.contains(&op.as_str()) || ["j", "jal", "tail"].contains(&op.as_str()))
                && let Some(d) = direct(args)
            {
                if body[0].0 <= d && d <= body.last().unwrap().0 && !is_call(op, args) {
                    continue;
                }
                if let Some(callee) = entries.get(&d) {
                    edges.get_mut(name).unwrap().insert(callee.clone());
                    continue;
                }
            }
            if target.is_none() {
                if let Some(targets) = transfers.get(name).and_then(|m| m.get(address)) {
                    for d in targets {
                        if body[0].0 <= *d && *d <= body.last().unwrap().0 {
                            continue;
                        }
                        let callee = entries.get(d);
                        if let Some(callee) = callee {
                            edges.get_mut(name).unwrap().insert(callee.clone());
                        } else {
                            frontier.push(json!({"caller":name,"address":format!("0x{address:x}"),"kind":"computed-external-target","target":format!("0x{d:x}"),"symbol":callee}));
                        }
                    }
                    continue;
                }
                frontier.push(json!({"caller":name,"address":format!("0x{address:x}"),"kind":if ["unimp","ebreak"].contains(&op.as_str()){"exception-transfer"}else{"unresolved-transfer"},"instruction":format!("{op} {}",args.join(", "))}));
                continue;
            }
            let target = target.as_ref().unwrap();
            let callee = re(r"\+0x[0-9a-f]+$").replace(target, "");
            if callee == name.split(" [0x").next().unwrap() && !is_call(op, args) {
                continue;
            }
            if selected.contains(callee.as_ref()) && target == callee.as_ref() {
                edges.get_mut(name).unwrap().insert(callee.into_owned());
            } else {
                frontier.push(json!({"caller":name,"address":format!("0x{address:x}"),"kind":"unmeasured-target","target":target}));
            }
        }
    }
    for (name, sites) in discovered {
        let body = instructions(&functions[name])?;
        for target in sites.values().flatten() {
            if body[0].0 <= *target && *target <= body.last().unwrap().0 {
                continue;
            }
            if let Some(callee) = entries.get(target) {
                edges.get_mut(name).unwrap().insert(callee.clone());
            }
        }
    }
    Ok((edges, frontier))
}
pub fn longest_path(
    root: &str,
    edges: &Edges,
    frames: &Map<String, u64>,
) -> Result<(u64, Vec<String>)> {
    fn visit(
        name: &str,
        edges: &Edges,
        frames: &Map<String, u64>,
        active: &mut Set<String>,
        memo: &mut Map<String, (u64, Vec<String>)>,
    ) -> Result<(u64, Vec<String>)> {
        ensure!(
            !active.contains(name),
            "recursive selected call graph: {name}"
        );
        if let Some(v) = memo.get(name) {
            return Ok(v.clone());
        }
        active.insert(name.into());
        let mut best = (0, vec![]);
        for child in &edges[name] {
            let result = visit(child, edges, frames, active, memo)?;
            if result.0 > best.0 || best.1.is_empty() {
                best = result;
            }
        }
        active.remove(name);
        best.0 += frames[name];
        best.1.insert(0, name.into());
        memo.insert(name.into(), best.clone());
        Ok(best)
    }
    let mut memo = Map::new();
    for name in edges.keys() {
        visit(name, edges, frames, &mut Set::new(), &mut memo)?;
    }
    Ok(memo[root].clone())
}

pub fn analyze(
    disassembly: &str,
    available: u64,
    readonly: Option<&Sections>,
    compiler: Option<&Map<String, crate::machine::Proof>>,
    extents: &Map<u32, u32>,
) -> Result<Value> {
    let functions = functions_from(disassembly, extents)?;
    let poll = reader_symbols(&functions)?;
    let selected: Set<_> = functions
        .keys()
        .filter(|n| {
            SCOPES
                .iter()
                .any(|s| n.trim_start_matches('<').starts_with(s))
                || **n == poll
        })
        .cloned()
        .collect();
    if let Some(c) = compiler {
        ensure!(
            c.keys().cloned().collect::<Set<_>>() == selected,
            "compiler coverage differs from the complete selected scope"
        );
    }
    let mut frames = Map::new();
    let mut dispatches: Map<String, Set<u32>> = Map::new();
    let mut unsupported = Map::new();
    let mut transfers = Map::new();
    let mut discovered = Map::new();
    for name in &selected {
        let result = (|| -> Result<u64> {
            let resolution = if let Some(sections) = readonly {
                resolve_transfers(&instructions(&functions[name])?, sections)?
            } else {
                TransferEvidence(Map::new(), Map::new())
            };
            transfers.insert(name.clone(), resolution.0);
            discovered.insert(name.clone(), resolution.1);
            if let Some(c) = compiler {
                dispatches.insert(
                    name.clone(),
                    crate::machine::corroborate(&functions[name], &c[name], &transfers[name])?,
                );
                Ok(c[name].size)
            } else {
                stack_frame(&functions[name], &transfers[name])
            }
        })();
        match result {
            Ok(size) => {
                frames.insert(name.clone(), size);
            }
            Err(e) => {
                unsupported.insert(name.clone(), e.to_string());
            }
        }
    }
    let (mut edges, mut frontier) = direct_graph(&functions, &selected, &transfers, &discovered)?;
    if let Some(c) = compiler {
        for name in &selected {
            edges.get_mut(name).unwrap().extend(c[name].callees.clone());
        }
        frontier.retain(|site| {
            !dispatches
                .get(site["caller"].as_str().unwrap())
                .is_some_and(|s| {
                    s.contains(&(number(site["address"].as_str().unwrap()).unwrap() as u32))
                })
        });
    }
    let mut reached = Set::new();
    let mut pending = vec![poll.clone()];
    while let Some(name) = pending.pop() {
        if reached.insert(name.clone()) {
            pending.extend(edges[&name].iter().cloned());
        }
    }
    let render_transfers = |data: &Map<String, Transfers>| -> Value {
        json!(
            data.iter()
                .filter(|(_, s)| !s.is_empty())
                .map(|(n, s)| (
                    n,
                    s.iter()
                        .map(|(a, t)| (
                            format!("0x{a:x}"),
                            t.iter().map(|v| format!("0x{v:x}")).collect::<Vec<_>>()
                        ))
                        .collect::<Map<_, _>>()
                ))
                .collect::<Map<_, _>>()
        )
    };
    let mut report = json!({"verdict":"BLOCKED_UNPROVEN","whole_program_bound":false,"available_bytes":available,"reserve_bytes":RESERVE,"entry_frames":null,"entry_required_bytes":null,"selected_path_bytes":null,"selected_path":null,"accounted_bytes_with_reserve":null,"selected_symbol_count":selected.len(),"unsupported_frames":unsupported,"frames":frames,"edges":edges,"computed_transfers":render_transfers(&transfers),"discovered_transfer_candidates":render_transfers(&discovered),"compiler_local_dispatches":dispatches.iter().filter(|(_,s)|!s.is_empty()).map(|(n,s)|(n,s.iter().map(|v|format!("0x{v:x}")).collect::<Vec<_>>())).collect::<Map<_,_>>(),"frontier":frontier,"selected_not_reached_by_direct_edges":selected.difference(&reached).collect::<Vec<_>>(),"unselected_symbol_count":functions.len()-selected.len(),"task_body_symbol":if functions.contains_key(TASK){"measured"}else{"absent; no independent measurement"},"scopes":SCOPES,"limitations":LIMITATIONS});
    if !unsupported.is_empty() {
        return Ok(report);
    }
    let entries: Map<String, u64> = Map::from([
        (
            "task".into(),
            frames[&poll] + frames.get(TASK).unwrap_or(&0),
        ),
        ("library".into(), frames[LIBRARY]),
        ("effects".into(), frames[EFFECT]),
    ]);
    let required = entries["task"] + entries["library"].max(entries["effects"]) + RESERVE;
    let (path_bytes, path) = longest_path(&poll, &edges, &frames)?;
    let accounted = required
        .max(path_bytes + RESERVE)
        .max(frames.values().max().unwrap() + RESERVE);
    report["verdict"] = json!(if entries["task"] > 4096 || accounted > available {
        "BLOCKED_BUDGET"
    } else {
        "PASS_LIMITED"
    });
    report["entry_frames"] = json!(entries);
    report["entry_required_bytes"] = json!(required);
    report["selected_path_bytes"] = json!(path_bytes);
    report["selected_path"] = json!(path);
    report["accounted_bytes_with_reserve"] = json!(accounted);
    Ok(report)
}

pub const LIMITATIONS: &[&str] = &[
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
];
