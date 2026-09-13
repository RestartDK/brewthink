# Reader memory evidence

## Stack evidence is limited

`scripts/check-reader-stack.py` is a regression gate for an ELF, not a whole-program worst-case stack proof. `PASS_LIMITED` means its selected frame sums fit the linked `.stack` section with the unchanged 8,192-byte allowance. The allowance does not prove that omitted work fits.

The gate selects emitted reader-app, reader-orchestration, book-resume, device-EPUB, ZIP-stream, scratch, and bounded-layout functions, plus the reader task poll. It reports each frame, direct edge, longest selected path, and selected function not reached through those edges. The historical task/library/effect check and 4,096-byte task-frame limit remain enforced. Missing required entry symbols fail instead of silently becoming zero-byte frames.

### Fixed frames and compiler-managed dispatch

The producer uses Rust 1.97.1 / LLVM 22.1.6. Two fresh, offline, locked production links emit different diagnostics:

- `-C 'remark=prologepilog stack-frame-layout'` reports finalized frame sizes, including explicit zero, and local slot layouts.
- `-C llvm-args=-print-before=riscv-asm-printer` reports the final machine instructions and CFG for every selected raw symbol.

The links must produce byte-identical entire ELFs. These options request diagnostics, not a differently optimized analysis program. Separate links avoid interleaving LLVM output with Rust diagnostics. The machine dump must come after late epilogue merging; an earlier post-prologue dump can legitimately have different SP-site counts.

Each selected symbol needs unique size/layout and machine records, a unique raw ELF symbol and executable extent, and disassembly covering every instruction byte. Missing, duplicate, truncated or unsupported records fail. Normal fixed locals have layout type `Variable`; that is not a variable-sized allocation. Dynamic/scalable slots, alignment beyond the RV32 ABI's 16 bytes, stack realignment, non-immediate SP changes and nonstandard save/restore helpers are rejected.

ELF `STT_FUNC` extents distinguish function boundaries from interior debug labels in the disassembler output. Interior labels keep their instructions in the enclosing function; real function entries and same-address aliases remain visible to the raw-symbol and complete-byte checks. Direct selected edges use instruction addresses rather than debug-label names.

The final machine CFG must account for every block and explicit branch target. Stack-depth propagation checks frame-setup/destroy adjustments, consistent joins, allocating cycles, balanced return/tail exits and the compiler's exact frame size. Local indirect dispatch needs closed compiler jump-table successors. Emitted SP adjustment multisets and indirect-transfer registers must match the machine record. A successful disassembly measurement cannot contradict the compiler frame, and known allocating cycles remain failures.

Inline assembly is not assumed harmless. Only the reader's four exact CSR templates and checked register operands are admitted. Unknown templates, memory accesses, control transfers and reserved-register aliases fail. A store below SP can exceed a frame without moving SP; checking SP writes alone would miss it.

This argument relies on the pinned compiler's lowering and well-defined source execution. It is not protection against corrupted coroutine tags, arbitrary RAM corruption or a compromised compiler. Unwind CFA records alone are not used as maximum-SP evidence.

### Selected paths and frontiers

The emitted-code scanner also propagates entry-relative SP depths and resolves finite targets from immutable ELF data. Joins union possible register values; exceeding its value bound becomes unknown, never an empty target set. Previously discovered selected callees remain in accounting even if a later predecessor makes the full target set unknown. Such a site keeps its unresolved frontier; partial candidates are not treated as complete CFG evidence.

Direct paths add full frames, including tail transfers, and can overcount mutually exclusive work. Calls assume ABI-balanced SP. Paths stop at unmeasured callees; callbacks from those helpers are not followed. Compiler-proven local dispatch is not an interprocedural frontier. Unresolved external transfers and traps remain explicit. Selected recursion fails, including disconnected cycles; recursion beyond that graph remains unmeasured.

`BLOCKED_UNPROVEN` leaves total accounting null when selected frames cannot be established. `BLOCKED_BUDGET` retains complete measurements that exceed the unchanged budget. Both fail. No absent or unsupported frame becomes zero. Other gaps include executor callers, interrupt nesting, other tasks, ROM and unselected assembly. `--require-complete` rejects this evidence even when the limited budget passes.

### Fresh artifacts and image construction

Run `python3 scripts/check-reader-memory.py artifacts/reader-memory-01` with a new, gitignored or external directory. The runner cleans only this package's release artifacts in the selected target directory before each link. It records source hashes, HEAD/diff, Cargo configuration, production commands, relevant environment, compiler identity, locked registry source contents, generated build/linker inputs, raw symbols/extents, complete diagnostic logs and both ELF hashes. Compiler, wrapper, and bootstrap overrides fail. Cargo configuration includes also fail rather than silently selecting an unrecorded tool. Cargo `[env]` supports only the recorded `DEFMT_LOG` setting, so configured rustup, path, and build overrides cannot change the compiler subprocess. `CARGO_HOME` must be absolute. Cached registry archives and vendored checksums are checked against `Cargo.lock`.

The frozen `reader.elf` is read-only. After the gate passes, `verified.json` binds that ELF, `inputs.json` and `stack.json`. `--verify-elf PATH` checks this completed passing bundle without rebuilding. Missing completion, changed reports, mismatched ELFs or changed source/build inputs fail. Reusing a report directory is refused.

Both `build-reader-app1.sh` and the generic builder's `reader-app` path package this exact proved ELF, not a subsequent Cargo rebuild. They recheck the completed evidence after image generation and retain the stack report beside the image. No build command flashes hardware. Runtime integration, source changes or changed generated inputs require fresh evidence.

LLVM implementation references: [frame finalization](https://github.com/llvm/llvm-project/blob/llvmorg-22.1.6/llvm/lib/CodeGen/PrologEpilogInserter.cpp), [frame layout remarks](https://github.com/llvm/llvm-project/blob/llvmorg-22.1.6/llvm/lib/CodeGen/StackFrameLayoutAnalysisPass.cpp), and [RISC-V frame lowering](https://github.com/llvm/llvm-project/blob/llvmorg-22.1.6/llvm/lib/Target/RISCV/RISCVFrameLowering.cpp). The ignored `--stack-size-section` experiment and nightly-only `-Z emit-stack-sizes` are not used; neither missing output nor a toolchain change is a substitute for evidence.

## Initialization validity and ownership

`Scratch<N>` owns an aligned byte allocation. Its exclusive mutable borrow prevents a typed workspace reference from surviving byte reuse. Compile-time assertions reject values that are too large, too aligned, or need Drop. The unsafe initializer must write a valid value before `Scratch::initialize` creates a reference. Returning to bytes clears the allocation because typed values can have uninitialized padding or inactive enum payloads.

The reader overlays `InflateWorkspace` and `DevicePublication` in `EpubWorkspace`. It projects raw field pointers, initializes both fields, and only then exposes references. `ContentWorkspace` similarly initializes `BookCatalog` or `BoundedPage`. Publication, catalog, and page initialization writes every field using typed writes. It does not assume that an `Option<T>` is all-zero. Existing constructor comparisons and scratch-transition tests exercise those initializers. No reader layout changes accompany this verification work.

`InflateWorkspace::initialize_in_place` has a different dependency contract. Zero bytes must already represent a valid `InflateState` before `reset` can borrow it. Resetting cannot repair an invalid Rust value after a reference has been formed.

The reviewed miniz_oxide version is exactly 0.9.1, with default features disabled. The relevant pinned sources are:

- [`inflate/core.rs`](https://docs.rs/crate/miniz_oxide/0.9.1/source/src/inflate/core.rs), `HuffmanTable` at lines 47-67, `DecompressorOxide` at 261-304, its all-zero default at 400-431, and `State::Start = 0` at 436-437. The remaining fields are integers and integer arrays.
- [`inflate/stream.rs`](https://docs.rs/crate/miniz_oxide/0.9.1/source/src/inflate/stream.rs), `InflateState` at 61-83. Its additional fields are a byte dictionary, integer offsets, booleans, `DataFormat`, and `TINFLStatus`.
- [`lib.rs`](https://docs.rs/crate/miniz_oxide/0.9.1/source/src/lib.rs), `DataFormat` at 157-165. Zero selects `Zlib`, which is valid but not the format the ZIP reader needs.
- [`inflate/mod.rs`](https://docs.rs/crate/miniz_oxide/0.9.1/source/src/inflate/mod.rs), `TINFLStatus::Done` at 53. Its explicit discriminant is zero.
- `inflate/stream.rs` at 23-55 and 152-154. `reset(Raw)` uses `FullReset`, clears the dictionary, initializes core state, resets offsets, sets `first_call`, clears `has_flushed`, and sets `NeedsMoreInput`.

The offline runner rejects changes to these four source files or the resolved version until the contract is reviewed. Compile-time checks also preserve the public zero discriminants and no-Drop condition. Hash checks freeze the reviewed source, not a formal proof of validity. The private-field validity argument still rests on source review. A dependency update requires that review before updating hashes, followed by the behavioral tests and an embedded release link.

## Executable controls

Host ZIP tests initialize poisoned storage and call `inflate` directly before the ZIP API can reset it. They compare statuses, consumed bytes, output bytes, and progress against the safe constructor with split input and output. They cover Raw format, Huffman decoding, dictionary wrap, successful reuse, sticky invalid/truncated-stream failures, full reset, and ZIP length/decompression failure recovery. The invalid-distance test checks that a previous dictionary cannot leak after reset.

The ownership tests compile valid reuse and reject oversized, over-aligned, Drop-requiring, and overlapping-borrow examples. They never run invalid pointers or deliberately form invalid references. This is host evidence, not a Miri run or a device-stack measurement.

The Python controls cover the emitted `LayoutError` formatter's conditional frame, historical overflow, exclusive branches, joins, balanced loops/calls, unknown and aliased SP writes, allocating cycles, explicit zero, malformed/duplicate records, hidden branch targets, stack-memory assembly, selected-edge retention, recursion and disconnected cycles. Producer controls reject stale/cross-ELF artifacts, altered sources, incomplete builds, changed reports, compiler-selection overrides and reused output directories.
