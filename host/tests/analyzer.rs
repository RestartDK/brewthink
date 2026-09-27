use anyhow::Result;
use brewthink_host::{machine, stack};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn decode(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(decode),
        Value::Object(items) => {
            if let Some(text) = items.get("bytes").and_then(Value::as_str) {
                *value = json!(
                    (0..text.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
                        .collect::<Vec<_>>()
                );
            } else {
                items.values_mut().for_each(decode)
            }
        }
        _ => {}
    }
}
fn arg<T: DeserializeOwned>(args: &[Value], i: usize) -> T {
    serde_json::from_value(args[i].clone()).unwrap()
}
fn optional<T: DeserializeOwned + Default>(args: &[Value], i: usize) -> T {
    args.get(i).map_or_else(T::default, |_| arg(args, i))
}
fn run(case: &Value) -> Result<Value> {
    let args = case["args"].as_array().unwrap();
    Ok(match case["op"].as_str().unwrap() {
        "stack.instructions" => {
            let parsed = json!(stack::instructions(&arg::<Vec<String>>(args, 0))?);
            json!(
                parsed
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|instruction| {
                        let operands = if instruction["operands"].as_array().unwrap().is_empty() {
                            json!([""])
                        } else {
                            instruction["operands"].clone()
                        };
                        json!([
                            instruction["address"],
                            instruction["opcode"],
                            operands,
                            instruction["target"]
                        ])
                    })
                    .collect::<Vec<_>>()
            )
        }
        "stack.stack_frame" => json!(stack::stack_frame(
            &arg::<Vec<String>>(args, 0),
            &optional(args, 1)
        )?),
        "stack.functions_from" => json!(stack::functions_from(
            &arg::<String>(args, 0),
            &optional(args, 1)
        )?),
        "stack.direct_graph" => json!(stack::direct_graph(
            &arg(args, 0),
            &arg(args, 1),
            &optional(args, 2),
            &optional(args, 3)
        )?),
        "stack.longest_path" => json!(stack::longest_path(
            &arg::<String>(args, 0),
            &arg(args, 1),
            &arg(args, 2)
        )?),
        "stack.function_extents" => json!(stack::function_extents(&arg::<Vec<u8>>(args, 0))?),
        "stack.check" => json!(arg::<stack::EntryFrames>(args, 0).check(arg(args, 1))?),
        "stack.analyze" => json!(stack::analyze(
            &arg::<String>(args, 0),
            arg(args, 1),
            args.get(2).map(|_| arg(args, 2)).as_ref(),
            None,
            &Default::default()
        )?),
        "stack.reader_symbols" => json!(stack::reader_symbols(&arg(args, 0))?),
        "stack.resolve_transfers" => {
            let source: Vec<(u32, String, Vec<String>, Option<String>)> = arg(args, 0);
            let body: Vec<_> = source
                .into_iter()
                .map(|(address, opcode, operands, target)| {
                    format!(
                        "{address:x}: {opcode} {}{}",
                        operands.join(", "),
                        target.map_or_else(String::new, |target| format!(" <{target}>"))
                    )
                })
                .collect();
            let evidence = stack::resolve_transfers(&stack::instructions(&body)?, &arg(args, 1))?;
            json!((evidence.complete, evidence.discovered))
        }
        "stack.readonly_sections" => json!(stack::readonly_sections(&arg::<Vec<u8>>(args, 0))?),
        "stack.branch_values" => json!(stack::branch_values(
            &arg::<String>(args, 0),
            &arg::<Vec<String>>(args, 1),
            &arg(args, 2),
            arg(args, 3)
        )),
        "stack.finite" => json!(stack::finite(arg::<Vec<i64>>(args, 0))),
        "machine.machine_frame" => json!(machine::machine_frame(
            &arg::<Vec<String>>(args, 0),
            &arg(args, 1)
        )?),
        "machine.corroborate" => json!(machine::corroborate(
            &arg::<Vec<String>>(args, 1),
            &arg(args, 2),
            &optional(args, 3)
        )?),
        "machine.frame_records" => json!(machine::frame_records(
            &arg::<Vec<String>>(args, 0),
            &arg(args, 1)
        )?),
        "machine.machine_records" => json!(machine::machine_records(
            &arg::<Vec<String>>(args, 0),
            &arg(args, 1)
        )?),
        "machine.benign_assembly" => json!(machine::benign_assembly(&arg::<String>(args, 0))?),
        op => panic!("unmapped baseline operation: {op}"),
    })
}
#[test]
fn frame_slot_lengths_cannot_wrap_into_the_signed_extent() {
    let source = [
        "note: x prologepilog (analysis): 0 stack bytes in function 'f'",
        "note: x stack-frame-layout (analysis):",
        "Function: f",
        "Offset: [SP+0], Type: Variable, Align: 1, Size: 18446744073709551615",
    ]
    .map(String::from);
    let error = machine::frame_records(&source, &["f".into()].into()).unwrap_err();
    assert!(error.to_string().contains("slot alignment/extent"));
}

#[test]
fn malformed_instruction_operands_return_errors() {
    for line in [
        "1000: addi sp",
        "1000: li a0",
        "1000: lw a0",
        "1000: bgeu a0",
    ] {
        let error = stack::stack_frame(&[line.into()], &Default::default()).unwrap_err();
        assert!(error.to_string().contains("operands"), "{line}: {error}");
    }
    assert!(stack::resolve_transfers(&[], &vec![]).is_err());
}

#[test]
fn overflowing_machine_block_targets_return_errors() {
    let body = [
        "bb.0:",
        "  successors: %bb.4294967296(0x80000000)",
        "  PseudoRET",
    ]
    .map(String::from);
    let error = machine::machine_frame(
        &body,
        &machine::Frame {
            size: 0,
            slots: vec![],
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("block target"), "{error}");
}

#[test]
fn compiler_fallback_errors_retain_their_kind_through_context() {
    for (body, expected) in [
        (
            vec!["100: addi sp, sp, -16", "104: jr a0"],
            stack::StackFrameError::UnresolvedTransfer { address: 0x104 },
        ),
        (
            vec!["100: jr a0", "104: sub sp, sp, a2", "108: ret"],
            stack::StackFrameError::UnreachableStackWrite { address: 0x104 },
        ),
    ] {
        let body: Vec<_> = body.into_iter().map(String::from).collect();
        let error = stack::stack_frame(&body, &Default::default())
            .unwrap_err()
            .context("additional compiler diagnostic context");
        assert_eq!(
            error.downcast_ref::<stack::StackFrameError>(),
            Some(&expected)
        );
    }
    let error =
        stack::stack_frame(&["100: sub sp, sp, a0".into()], &Default::default()).unwrap_err();
    assert!(error.downcast_ref::<stack::StackFrameError>().is_none());
}

#[test]
fn signed_comparison_is_stack_neutral_but_cannot_define_the_stack_pointer() {
    for (instruction, valid) in [
        ("renamable $x9 = SLT $x0, renamable $x14", true),
        ("$x2 = SLT $x0, renamable $x14", false),
        ("$sp = SLT $x0, renamable $x14", false),
        ("renamable $x2 = SLT $x0, renamable $x14", false),
        ("$x9 = SLT $x0, $x14, implicit-def $x2", false),
    ] {
        let body = vec![
            "bb.0:".into(),
            "  $x2 = frame-setup ADDI $x2, -16".into(),
            format!("  {instruction}"),
            "  $x2 = frame-destroy ADDI $x2, 16".into(),
            "  PseudoRET".into(),
        ];
        let proof = machine::machine_frame(
            &body,
            &machine::Frame {
                size: 16,
                slots: vec![],
            },
        );
        assert_eq!(proof.is_ok(), valid, "{instruction}: {proof:?}");
        if let Ok(proof) = proof {
            assert_eq!(proof.size, 16);
            assert_eq!(proof.sp_writes, [(-16, 1), (16, 1)].into());
        }
    }
}

#[test]
fn image_storage_and_codec_owners_remain_in_the_measured_call_graph() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/analyzer.json")).unwrap();
    let case = cases.iter().find(|case| case["test"] == "test_reader_stack.ReaderStackTests.test_integrated_orchestration_and_identity_frames_are_selected").unwrap();
    let encoded = serde_json::to_string(case).unwrap();
    for owner in [
        "brewthink::storage::catalog::prepare_image",
        "brewthink::image_cache::prepare",
        "brewthink::image_decoder::decode",
        "brewthink::reader::render",
        "embedded_sdmmc::allocate",
        "tjpgd_rs::decode",
        "miniz_oxide::inflate",
    ] {
        let mut case: Value = serde_json::from_str(
            &encoded.replace("brewthink::reader_orchestration::drive_effect", owner),
        )
        .unwrap();
        decode(&mut case);
        assert_eq!(run(&case).unwrap(), case["result"], "{owner}");
    }
}

#[test]
fn preserved_analyzer_cases() {
    let mut cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/analyzer.json")).unwrap();
    let mut failures = vec![];
    for (index, case) in cases.iter_mut().enumerate() {
        decode(case);
        let result = run(case);
        let mismatch = if case.get("error").is_some() {
            match &result {
                Err(error) => case["error_pattern"].as_str().is_some_and(|pattern| {
                    !regex::Regex::new(pattern)
                        .unwrap()
                        .is_match(&error.to_string())
                }),
                Ok(_) => true,
            }
        } else {
            result
                .as_ref()
                .map_or(true, |result| *result != case["result"])
        };
        if mismatch {
            failures.push(format!(
                "{index}: {} / {}\nexpected: {}\nactual: {result:?}",
                case["test"],
                case["op"],
                case.get("result").unwrap_or(&case["error"])
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
