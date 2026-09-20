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
        "stack.instructions" => json!(stack::instructions(&arg::<Vec<String>>(args, 0))?),
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
        "stack.check" => json!(stack::check(&arg(args, 0), arg(args, 1))?),
        "stack.analyze" => json!(stack::analyze(
            &arg::<String>(args, 0),
            arg(args, 1),
            args.get(2).map(|_| arg(args, 2)).as_ref(),
            None,
            &Default::default()
        )?),
        "stack.reader_symbols" => json!(stack::reader_symbols(&arg(args, 0))?),
        "stack.resolve_transfers" => json!(stack::resolve_transfers(
            &arg::<Vec<stack::Instruction>>(args, 0),
            &arg(args, 1)
        )?),
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
