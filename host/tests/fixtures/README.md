# Analyzer migration corpus

`analyzer.json` records 178 public-function calls made by the 81 stack and compiler-evidence tests at `41dc639c1dc8b38c0f24981b7cafb3702e2d1c91`. The original tests passed while these calls were recorded.

Each record contains the original test name, operation, arguments, and either the exact returned value or the expected rejection. `error_pattern` retains the original test's `assertRaisesRegex` requirement. A null pattern means the original test required rejection without matching its message. Byte arrays are hex-encoded; sets are sorted arrays.

`host/tests/analyzer.rs` executes these inputs against the Rust implementation, compares full results, and checks rejection patterns. No Python interpreter or original implementation participates in this suite. New parser behavior needs explicit Rust cases; do not regenerate expected results from the implementation under test.
