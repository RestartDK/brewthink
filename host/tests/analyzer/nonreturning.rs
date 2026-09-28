use super::*;

fn provenance(extra: &str) -> machine::Proof {
    let text = format!(
        "bb.0:\n  successors: %bb.1(0x40000000), %bb.2(0x40000000)\n  BEQ $x10, $x0, %bb.2\nbb.1:\n  $x2 = frame-setup ADDI $x2, -16\n  PseudoCALL target-flags(riscv-call) @fatal, <regmask $x0>, implicit-def $x1, implicit-def $x2\nbb.2:\n{extra}  PseudoRET\n"
    );
    machine::machine_frame(
        &text.lines().map(str::to_owned).collect::<Vec<_>>(),
        &machine::Frame {
            size: 16,
            slots: vec![],
        },
    )
    .unwrap()
}

fn assembly() -> Vec<String> {
    [
        "1000: beqz a0, 0x1010",
        "1004: addi sp, sp, -16",
        "1008: call 0x2000 <fatal>",
        "100c: bnez a0, 0x1004",
        "1010: ret",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn a_proven_nonreturning_call_does_not_create_an_artificial_allocating_loop() {
    let proof = provenance("");
    assert_eq!(proof.nonreturning_calls, [("fatal".into(), 1)].into());
    let body = assembly();
    assert!(stack::stack_frame(&body, &Default::default()).is_err());
    assert!(machine::corroborate(&body, &proof, &Default::default()).is_ok());
    let mut missing = proof.clone();
    missing.nonreturning_calls.clear();
    assert!(machine::corroborate(&body, &missing, &Default::default()).is_err());
    for mutated in [
        body.iter()
            .map(|line| line.replace("<fatal>", "<other>"))
            .collect::<Vec<_>>(),
        body.iter()
            .map(|line| line.replace("100c: bnez a0, 0x1004", "100c: call 0x2000 <fatal>"))
            .collect(),
        body.iter()
            .map(|line| line.replace("sp, sp, -16", "sp, sp, -32"))
            .collect(),
    ] {
        assert!(machine::corroborate(&mutated, &proof, &Default::default()).is_err());
    }
    let mut real_loop = body;
    real_loop.insert(2, "1006: bnez a1, 0x1004".into());
    assert!(machine::corroborate(&real_loop, &proof, &Default::default()).is_err());
}

#[test]
fn emitted_xml_tag_failure_path_is_bounded_by_matching_compiler_call_provenance() {
    let machine = include_str!("../fixtures/xml-tag-noreturn.mir")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let assembly = include_str!("../fixtures/xml-tag-noreturn.asm")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let proof = machine::machine_frame(
        &machine,
        &machine::Frame {
            size: 16,
            slots: vec![(-4, "Spill".into(), 4, 4), (-8, "Spill".into(), 4, 4)],
        },
    )
    .unwrap();
    assert_eq!(proof.nonreturning_calls.len(), 1);
    assert!(stack::stack_frame(&assembly, &Default::default()).is_err());
    assert!(machine::corroborate(&assembly, &proof, &Default::default()).is_ok());
}

#[test]
fn mixed_returning_and_nonreturning_calls_do_not_authorize_name_based_terminal_edges() {
    let proof = provenance(
        "  PseudoCALL target-flags(riscv-call) @fatal, <regmask $x0>, implicit-def $x1, implicit-def $x2\n",
    );
    assert!(proof.nonreturning_calls.is_empty());
}
