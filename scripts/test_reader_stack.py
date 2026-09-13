import importlib.util
from pathlib import Path
import struct
import unittest

spec = importlib.util.spec_from_file_location("reader_stack", Path(__file__).with_name("check-reader-stack.py"))
stack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stack)


class TableTransferTests(unittest.TestCase):
    BODY = [
        "1000: addi sp, sp, -16", "1004: lbu a0, 0x0(a1)",
        "1008: slli a0, a0, 2", "100c: li a2, 0x2000",
        "1010: add a0, a0, a2", "1014: lw a0, 0x0(a0)",
        "1018: jr a0", "101c: addi sp, sp, 16", "1020: ret",
    ]

    def table(self, targets=None):
        return [(0x2000, struct.pack("<256I", *(targets or [0x101c] * 256)))]

    def test_byte_index_uses_every_possible_elf_word(self):
        targets = [0x101c] * 255 + [0x3000]
        transfers = stack.resolve_transfers(stack.instructions(self.BODY), self.table(targets)).complete
        self.assertEqual(transfers, {0x1018: {0x101c, 0x3000}})
        self.assertEqual(stack.stack_frame(self.BODY, transfers), 16)

    def test_missing_or_partial_table_cannot_prove_a_live_frame(self):
        for readonly in ([], [(0x2000, b"\x1c\x10\x00\x00")]):
            transfers = stack.resolve_transfers(stack.instructions(self.BODY), readonly).complete
            self.assertEqual(transfers, {})
            with self.assertRaisesRegex(ValueError, "no frame proof"):
                stack.stack_frame(self.BODY, transfers)

    def test_computed_allocating_cycle_is_rejected(self):
        targets = [0x101c] * 255 + [0x1000]
        transfers = stack.resolve_transfers(stack.instructions(self.BODY), self.table(targets)).complete
        with self.assertRaisesRegex(ValueError, "cycle"):
            stack.stack_frame(self.BODY, transfers)

    def test_dynamic_sp_behind_a_table_is_not_a_zero_frame(self):
        body = [line.replace("addi sp, sp, 16", "sub sp, sp, a3") for line in self.BODY]
        transfers = stack.resolve_transfers(stack.instructions(body), self.table()).complete
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame(body, transfers)

    def test_corrupt_internal_table_target_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "unparsed instruction"):
            stack.resolve_transfers(stack.instructions(self.BODY), self.table([0x101e] * 256))

    def test_call_clobbers_a_computed_target(self):
        body = self.BODY[:6] + ["1016: call 0x4000 <external>"] + self.BODY[6:]
        self.assertEqual(stack.resolve_transfers(stack.instructions(body), self.table()).complete, {})

    def test_unsigned_guard_bounds_unknown_word_indices(self):
        body = ["1000: li a1, 3", "1004: bltu a1, a0, 0x1010", "1008: jr a0", "1010: ret"]
        environments = {"zero": frozenset({0}), "a1": frozenset({3})}
        self.assertEqual(stack.branch_values("bltu", ["a1", "a0"], environments, False)["a0"], {0, 1, 2, 3})
        self.assertEqual(stack.resolve_transfers(stack.instructions(body), []).complete, {0x1008: {0, 2}})

    def test_branch_to_following_instruction_does_not_drop_taken_values(self):
        body = ["1000: li a0, 7", "1004: li a1, 3", "1008: bltu a1, a0, 0x100c", "100c: jr a0"]
        self.assertEqual(stack.resolve_transfers(stack.instructions(body), []).complete, {0x100c: {6}})

    def test_join_unions_computed_targets(self):
        body = ["1000: beqz a1, 0x100c", "1004: li a0, 0x1014", "1008: j 0x1010",
                "100c: li a0, 0x1018", "1010: jr a0", "1014: ret", "1018: ret"]
        self.assertEqual(stack.resolve_transfers(stack.instructions(body), []).complete, {0x1010: {0x1014, 0x1018}})

    def test_second_dispatch_is_discovered_after_first(self):
        body = ["1000: li a0, 0x1008", "1004: jr a0", "1008: li a0, 0x1010", "100c: jr a0", "1010: ret"]
        self.assertEqual(stack.resolve_transfers(stack.instructions(body), []).complete, {0x1004: {0x1008}, 0x100c: {0x1010}})

    def test_value_limit_degrades_to_unknown_not_an_empty_set(self):
        self.assertIsNone(stack.finite(range(stack.VALUE_LIMIT + 1)))
        self.assertEqual(stack.finite([-1, 1 << 32]), {0xffffffff, 0})

    def test_computed_selected_callee_keeps_full_frame_accounting(self):
        poll = f"TaskStorage<{stack.TASK}>::poll"
        disassembly = "\n".join([
            f"1000 <{poll}>:", *self.BODY[:7], f"3000 <{stack.LIBRARY}>:", "3000: ret",
            f"4000 <{stack.EFFECT}>:", "4000: addi sp, sp, -32", "4004: ret",
            "5000 <external_data>:", "5000: 00 00 00 00 ....",
        ])
        report = stack.analyze(disassembly, 20000, self.table([0x4000] * 256))
        self.assertEqual(report["verdict"], "PASS_LIMITED")
        self.assertEqual(report["edges"][poll], [stack.EFFECT])
        self.assertEqual(report["selected_path_bytes"], 48)

    def test_discovered_selected_edge_survives_later_unknown_predecessor(self):
        body = ["1000: beqz a0, 0x1010", "1002: li a5, 0x2000", "1004: j 0x1030",
                "1010: li t0, 0x1020", "1012: jr t0", "1020: mv a5, a1",
                "1022: j 0x1030", "1030: jr a5"]
        resolution = stack.resolve_transfers(stack.instructions(body), [])
        self.assertNotIn(0x1030, resolution.complete)
        self.assertEqual(resolution.discovered[0x1030], {0x2000})
        edges, frontier = stack.direct_graph({"A": body, "B": ["2000: ret"]}, {"A", "B"},
                                              {"A": resolution.complete}, {"A": resolution.discovered})
        self.assertEqual(edges["A"], {"B"})
        self.assertTrue(any(site["address"] == "0x1030" and site["kind"] == "unresolved-transfer" for site in frontier))
        self.assertEqual(stack.longest_path("A", edges, {"A": 16, "B": 400})[0], 416)

    def elf(self, flags=2):
        elf = bytearray(52 + 40 + 4)
        elf[:7] = b"\x7fELF\x01\x01\x01"
        struct.pack_into("<H", elf, 18, 243)
        struct.pack_into("<I", elf, 32, 52)
        struct.pack_into("<HH", elf, 46, 40, 1)
        struct.pack_into("<10I", elf, 52, 0, 1, flags, 0x2000, 92, 4, 0, 0, 4, 0)
        elf[92:] = b"\x1c\x10\x00\x00"
        return elf

    def test_only_immutable_allocated_elf_bytes_can_resolve_loads(self):
        self.assertEqual(stack.readonly_sections(self.elf()), [(0x2000, b"\x1c\x10\x00\x00")])
        for flags in (0, 1, 3, 6):
            with self.assertRaisesRegex(ValueError, "no immutable"):
                stack.readonly_sections(self.elf(flags))

    def test_truncated_or_wrong_elf_is_rejected(self):
        for data in (b"", self.elf()[:51], self.elf()[:-1], b"X" + self.elf()[1:]):
            with self.assertRaises(ValueError):
                stack.readonly_sections(data)


class FunctionBoundaryTests(unittest.TestCase):
    def elf(self, symbols, entry_size=16):
        data = bytearray(224 + len(symbols) * 16)
        data[:7] = b"\x7fELF\x01\x01\x01"
        struct.pack_into("<H", data, 18, 243)
        struct.pack_into("<I", data, 32, 52)
        struct.pack_into("<HH", data, 46, 40, 4)
        struct.pack_into("<10I", data, 92, 0, 1, 2, 0x2000, 212, 4, 0, 0, 4, 0)
        struct.pack_into("<10I", data, 132, 0, 1, 6, 0x1000, 216, 8, 0, 0, 4, 0)
        struct.pack_into("<10I", data, 172, 0, 2, 0, 0, 224, len(symbols) * 16, 0, 0, 4, entry_size)
        for index, (address, size, kind) in enumerate(symbols):
            struct.pack_into("<IIIBBH", data, 224 + index * 16, 0, address, size, kind, 0, 2)
        return data

    def test_only_function_symbols_define_function_boundaries(self):
        elf = self.elf([(0x1000, 8, 2), (0x1004, 0, 0)])
        self.assertEqual(stack.function_extents(elf), {0x1000: 8})
        elf = self.elf([(0x1000, 8, 2), (0x1004, 0, 2)])
        self.assertEqual(stack.function_extents(elf), {0x1000: 8, 0x1004: 0})

    def test_zero_sized_aliases_do_not_discard_the_known_function_extent(self):
        symbols = [(0x1000, 8, 2), (0x1000, 0, 2)]
        for ordered in (symbols, list(reversed(symbols))):
            self.assertEqual(stack.function_extents(self.elf(ordered)), {0x1000: 8})
        functions = stack.functions_from("1000 <owner>:\n1000 <alias>:\n1000: ret\n", {0x1000: 8})
        self.assertEqual(set(functions), {"owner", "alias"})

    def test_malformed_function_metadata_fails_closed(self):
        cases = [
            self.elf([(0x1000, 8, 2)], entry_size=15),
            self.elf([(0x1000, 8, 2)])[:-1],
            self.elf([(0x1000, 12, 2)]),
            self.elf([(0x1000, 8, 2), (0x1000, 4, 2)]),
            self.elf([(0x1000, 0, 0)]),
        ]
        for elf in cases:
            with self.subTest(elf=bytes(elf)), self.assertRaises(ValueError):
                stack.function_extents(elf)

    def test_debug_labels_do_not_split_an_emitted_function(self):
        text = """1000 <brewthink::bounded_layout::layout>:
1000: addi sp, sp, -16
1004 <.L0 >:
1004: addi sp, sp, 16
1008 <another_local_label>:
1008: ret
100c <next>:
100c: ret
"""
        functions = stack.functions_from(text, {0x1000: 12, 0x100c: 4})
        self.assertEqual(set(functions), {"brewthink::bounded_layout::layout", "next"})
        self.assertEqual(len(stack.instructions(functions["brewthink::bounded_layout::layout"])), 3)
        self.assertEqual(stack.stack_frame(functions["brewthink::bounded_layout::layout"]), 16)

    def test_a_real_function_inside_an_extent_is_not_discarded_as_a_label(self):
        text = "1000 <outer>:\n1000: nop\n1004 <inner>:\n1004: ret\n"
        functions = stack.functions_from(text, {0x1000: 8, 0x1004: 4})
        self.assertEqual(set(functions), {"outer", "inner"})
        self.assertEqual(len(stack.instructions(functions["outer"])), 1)

    def test_labels_at_an_extent_end_do_not_extend_its_body(self):
        text = "1000 <owner>:\n1000: ret\n1004 <.L0 >:\n1004: nop\n"
        functions = stack.functions_from(text, {0x1000: 4})
        self.assertEqual(len(stack.instructions(functions["owner"])), 1)
        self.assertIn(".L0 ", functions)

    def test_direct_selected_edges_use_addresses_not_debug_label_names(self):
        functions = {
            "caller": ["1000: addi sp, sp, -16", "1004: j 0x2000 <.L0 >"],
            "callee": ["2000: addi sp, sp, -400", "2004: ret"],
        }
        edges, _ = stack.direct_graph(functions, set(functions))
        self.assertEqual(edges["caller"], {"callee"})
        self.assertEqual(stack.longest_path("caller", edges, {"caller": 16, "callee": 400})[0], 416)

    def test_direct_call_to_self_stays_recursive_with_a_debug_label(self):
        functions = {"caller": ["1000: addi sp, sp, -16", "1004: jal 0x1000 <.L0 >"]}
        edges, _ = stack.direct_graph(functions, set(functions))
        self.assertEqual(edges["caller"], {"caller"})
        with self.assertRaises(ValueError):
            stack.longest_path("caller", edges, {"caller": 16})


class ReaderStackTests(unittest.TestCase):
    def test_integrated_orchestration_and_identity_frames_are_selected(self):
        poll = f"TaskStorage<{stack.TASK}>::poll"
        orchestration = "brewthink::reader_orchestration::drive_effect"
        identity = "brewthink::storage::book_resume::SavedResume::decode"
        nodes = [
            (0x1000, poll, 48, (0x3000, stack.EFFECT)),
            (0x2000, stack.LIBRARY, 16, None),
            (0x3000, stack.EFFECT, 64, (0x4000, orchestration)),
            (0x4000, orchestration, 32, (0x5000, identity)),
            (0x5000, identity, 80, None),
        ]
        lines = []
        for address, name, frame, callee in nodes:
            lines.extend([f"{address:x} <{name}>:", f" {address:x}: addi sp, sp, -{frame}"])
            if callee:
                lines.append(f" {address + 4:x}: jal 0x{callee[0]:x} <{callee[1]}>")
            lines.append(f" {address + 8:x}: ret")
        report = stack.analyze("\n".join(lines), 20000)
        self.assertEqual(report["selected_path"], [poll, stack.EFFECT, orchestration, identity])
        self.assertEqual(report["selected_path_bytes"], 224)

    def test_emitted_layout_error_formatter_conditional_frame(self):
        body = """
42077b96: lbu a2, 0x0(a0)
42077b9a: sltiu a3, a2, 0x5
42077b9e: addi a2, a2, -0x4
42077ba0: addi a3, a3, -0x1
42077ba2: and a2, a2, a3
42077ba4: beqz a2, 0x42077bbe <<brewthink::bounded_layout::LayoutError as core::fmt::Debug>::fmt+0x28>
42077ba6: li a0, 0x1
42077ba8: bne a2, a0, 0x42077bf4 <<brewthink::bounded_layout::LayoutError as core::fmt::Debug>::fmt+0x5e>
42077bac: lw a2, 0x4(a1)
42077bae: lw a0, 0x0(a1)
42077bb0: lw a5, 0xc(a2)
42077bb2: lui a1, 0x3c044
42077bb6: addi a1, a1, 0x53f
42077bba: li a2, 0xf
42077bbc: jr a5
42077bbe: addi sp, sp, -0x10
42077bc0: sw ra, 0xc(sp)
42077bc2: sw s0, 0x8(sp)
42077bc4: addi s0, sp, 0x10
42077bc6: sw a0, -0xc(s0)
42077bca: lui a5, 0x3c044
42077bce: addi a5, a5, 0x33f
42077bd2: lui a4, 0x4206c
42077bd6: addi a4, a4, 0x2f0
42077bda: li a2, 0x3
42077bdc: addi a3, s0, -0xc
42077be0: mv a0, a1
42077be2: mv a1, a5
42077be4: auipc ra, 0xe
42077be8: jalr -0x298(ra) <<core::fmt::Formatter>::debug_tuple_field1_finish>
42077bec: lw ra, 0xc(sp)
42077bee: lw s0, 0x8(sp)
42077bf0: addi sp, sp, 0x10
42077bf2: ret
42077bf4: lw a2, 0x4(a1)
42077bf6: lw a0, 0x0(a1)
42077bf8: lw a5, 0xc(a2)
42077bfa: lui a1, 0x3c044
42077bfe: addi a1, a1, 0x54e
42077c02: li a2, 0xc
42077c04: jr a5
""".splitlines()
        self.assertEqual(stack.stack_frame(body), 16)

    def test_unsupported_selected_frame_is_reported_not_zero(self):
        poll = f"TaskStorage<{stack.TASK}>::poll"
        disassembly = "\n".join([
            f"100 <{poll}>:", "100: ret",
            f"200 <{stack.LIBRARY}>:", "200: sub sp, sp, a0", "204: ret",
            f"300 <{stack.EFFECT}>:", "300: ret",
        ])
        report = stack.analyze(disassembly, 20000)
        self.assertEqual(report["verdict"], "BLOCKED_UNPROVEN")
        self.assertFalse(report["whole_program_bound"])
        self.assertNotIn(stack.LIBRARY, report["frames"])
        self.assertIn(stack.LIBRARY, report["unsupported_frames"])
        self.assertIsNone(report["accounted_bytes_with_reserve"])
        self.assertEqual(len(report["frames"]) + len(report["unsupported_frames"]), report["selected_symbol_count"])

    def test_over_budget_measurements_are_preserved(self):
        poll = f"TaskStorage<{stack.TASK}>::poll"
        disassembly = "\n".join([
            f"100 <{poll}>:", "100: addi sp, sp, -16", f"104: jal 0x300 <{stack.EFFECT}>", "108: ret",
            f"200 <{stack.LIBRARY}>:", "200: ret",
            f"300 <{stack.EFFECT}>:", "300: addi sp, sp, -32", "304: ret",
        ])
        report = stack.analyze(disassembly, 8200)
        self.assertEqual(report["verdict"], "BLOCKED_BUDGET")
        self.assertEqual(report["accounted_bytes_with_reserve"], 8240)
        self.assertEqual(report["selected_path_bytes"], 48)
        self.assertFalse(report["whole_program_bound"])
        self.assertEqual(report["unsupported_frames"], {})

    def test_two_part_riscv_prologue(self):
        body = [
            "4204e38a: addi sp, sp, -0x100",
            "4204e38c: sw ra, 0xfc(sp)",
            "4204e3a8: lui a2, 0x9",
            "4204e3aa: addi a2, a2, 0x190",
            "4204e3ae: sub sp, sp, a2",
            "4204e3ee: ret",
        ]
        self.assertEqual(stack.stack_frame(body), 37520)

    def test_rejects_the_measured_startup_overflow(self):
        with self.assertRaisesRegex(ValueError, "budget exceeded"):
            stack.check({"task": 37536, "library": 41872, "effects": 0}, 62280)

    def test_accepts_separate_entry_frames(self):
        required = stack.check({"task": 1104, "library": 41872, "effects": 37504}, 62280)
        self.assertEqual(required, 51168)

    def test_bounded_allocation_after_branch_is_included(self):
        self.assertEqual(stack.stack_frame([
            "100: addi sp, sp, -16",
            "104: beqz a0, 0x110",
            "108: addi sp, sp, -4096",
            "110: ret",
        ]), 4112)

    def test_loop_cannot_reenter_an_allocating_prologue(self):
        with self.assertRaisesRegex(ValueError, "stack-depth cycle"):
            stack.stack_frame(["100: addi sp, sp, -16", "104: j 0x100 <f>"])

    def test_unknown_stack_write_after_call_is_not_ignored(self):
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame([
                "100: addi sp, sp, -16",
                "104: jalr a0",
                "108: sub sp, sp, a2",
            ])

    def test_unknown_stack_adjustments_fail_closed(self):
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame(["4204e3ae: sub sp, sp, a2"])
        with self.assertRaisesRegex(ValueError, "empty disassembly"):
            stack.stack_frame([])

    def test_leaf_zero_is_measured_not_missing(self):
        self.assertEqual(stack.stack_frame(["100: ret"]), 0)

    def test_positive_epilogue_after_control_flow(self):
        self.assertEqual(stack.stack_frame([
            "100: addi sp, sp, -16", "104: jalr a0",
            "108: addi sp, sp, 16", "10c: ret",
        ]), 16)

    def test_missing_effect_is_not_assumed_inlined(self):
        poll = "TaskStorage<" + stack.TASK + ">::poll"
        with self.assertRaisesRegex(ValueError, "required symbol missing"):
            stack.reader_symbols({poll: [], stack.LIBRARY: []})

    def test_duplicate_symbols_are_not_overwritten(self):
        functions = stack.functions_from("100 <f>:\n100: ret\n200 <f>:\n200: ret")
        self.assertEqual(set(functions), {"f [0x100]", "f [0x200]"})
        self.assertEqual(functions["f [0x100]"], ["100: ret"])
        with self.assertRaisesRegex(ValueError, "duplicate symbol and address"):
            stack.functions_from("100 <f>:\n100: ret\n100 <f>:\n100: ret\n100 <f>:\n100: ret")

    def test_unparsed_opcode_fails_instead_of_disappearing(self):
        with self.assertRaisesRegex(ValueError, "unparsed instruction"):
            stack.stack_frame(["100: <unknown>"])

    def test_indirect_and_unmeasured_calls_are_visible(self):
        functions = {"root": [
            "100: jalr a0", "104: jal 0x300 <outside>",
            "108: jalr 0x10(ra) <child>", "10c: ret",
        ], "child": ["200: ret"]}
        edges, frontier = stack.direct_graph(functions, set(functions))
        self.assertEqual(edges["root"], {"child"})
        self.assertEqual([site["kind"] for site in frontier], ["unresolved-transfer", "unmeasured-target"])

    def test_nested_frames_are_added_not_maximized_individually(self):
        self.assertEqual(stack.longest_path("a", {"a": {"b"}, "b": {"c"}, "c": set()}, {"a": 16, "b": 32, "c": 64}), (112, ["a", "b", "c"]))

    def test_recursive_and_disconnected_cycles_fail_closed(self):
        for edges in ({"a": {"a"}}, {"a": set(), "b": {"c"}, "c": {"b"}}):
            with self.assertRaisesRegex(ValueError, "recursive"):
                stack.longest_path("a", edges, {name: 16 for name in edges})

    def test_internal_branch_is_not_recursion_but_self_call_is(self):
        functions = {"a": ["100: j 0x100 <a>", "104: jal 0x100 <a>"]}
        edges, frontier = stack.direct_graph(functions, {"a"})
        self.assertEqual(edges, {"a": {"a"}})
        self.assertEqual(frontier, [])

    def test_branch_target_invalidates_linear_register_assumptions(self):
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame([
                "100: addi sp, sp, -16", "104: beqz a0, 0x110 <f+0x10>",
                "108: li a2, 16", "110: add sp, sp, a2", "114: ret",
            ])

    def test_mutually_exclusive_allocations_take_maximum_not_sum(self):
        self.assertEqual(stack.stack_frame([
            "100: beqz a0, 0x110", "104: addi sp, sp, -16", "108: j 0x114",
            "110: addi sp, sp, -32", "114: ret",
        ]), 32)

    def test_epilogue_on_one_branch_cannot_cancel_another_allocation(self):
        self.assertEqual(stack.stack_frame([
            "100: addi sp, sp, -16", "104: beqz a0, 0x114",
            "108: addi sp, sp, 16", "10c: j 0x118",
            "114: addi sp, sp, -32", "118: addi sp, sp, -64", "11c: ret",
        ]), 112)

    def test_equal_constants_survive_a_join(self):
        self.assertEqual(stack.stack_frame([
            "100: beqz a0, 0x110", "104: li a2, 32", "108: j 0x114",
            "110: li a2, 32", "114: sub sp, sp, a2", "118: ret",
        ]), 32)

    def test_conflicting_branch_constants_are_not_combined(self):
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame([
                "100: beqz a0, 0x110", "104: li a2, 16", "108: j 0x114",
                "110: li a2, 32", "114: sub sp, sp, a2", "118: ret",
            ])

    def test_call_clobbers_a_previously_known_adjustment(self):
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame([
                "100: li a2, 16", "104: jalr a0", "108: sub sp, sp, a2", "10c: ret",
            ])

    def test_balanced_call_inside_a_loop_preserves_frame(self):
        self.assertEqual(stack.stack_frame([
            "100: addi sp, sp, -16", "104: jalr a0", "108: bnez a0, 0x104",
            "10c: addi sp, sp, 16", "110: ret",
        ]), 16)

    def test_balanced_stack_allocation_cycle_has_finite_peak(self):
        self.assertEqual(stack.stack_frame([
            "100: addi sp, sp, -16", "104: addi sp, sp, 16",
            "108: bnez a0, 0x100", "10c: ret",
        ]), 16)

    def test_conditional_allocating_cycle_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "stack-depth cycle"):
            stack.stack_frame([
                "100: beqz a0, 0x10c", "104: addi sp, sp, -16",
                "108: j 0x100", "10c: ret",
            ])

    def test_loop_varying_adjustment_is_unknown(self):
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame([
                "100: li a2, 16", "104: addi a2, a2, 16",
                "108: bnez a0, 0x104", "10c: sub sp, sp, a2", "110: ret",
            ])

    def test_absolute_and_dynamic_sp_assignments_are_not_entry_offsets(self):
        for instruction in ("li sp, -16", "mv sp, a0", "and sp, sp, a0", "lw sp, 0(a0)", "addi x2, x2, -16"):
            with self.subTest(instruction=instruction), self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
                stack.stack_frame([f"100: {instruction}", "104: ret"])

    def test_unsupported_mnemonic_cannot_hide_implicit_stack_writes(self):
        for instruction in ("c.addi16sp -16", "unknown a0", "mret"):
            with self.subTest(instruction=instruction), self.assertRaisesRegex(ValueError, "unsupported instruction"):
                stack.stack_frame([f"100: {instruction}", "104: ret"])

    def test_emitted_illegal_instruction_ends_path_but_not_exception_evidence(self):
        body = ["100: addi sp, sp, -16", "104: unimp"]
        self.assertEqual(stack.stack_frame(body), 16)
        _, frontier = stack.direct_graph({"a": body}, {"a"})
        self.assertEqual(frontier[0]["kind"], "exception-transfer")
        with self.assertRaisesRegex(ValueError, "unreachable stack write"):
            stack.stack_frame(body + ["108: sub sp, sp, a0"])

    def test_zero_register_alias_cannot_create_a_false_constant(self):
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame([
                "100: li x0, 16", "104: sub sp, sp, x0", "108: ret",
            ])

    def test_unresolved_live_frame_transfer_is_not_assumed_to_exit(self):
        for instruction in ("jr a0", "jalr zero, a0, 0x0"):
            with self.subTest(instruction=instruction), self.assertRaisesRegex(ValueError, "unresolved in-frame transfer"):
                stack.stack_frame(["100: addi sp, sp, -16", f"104: {instruction}"])

    def test_unknown_transfer_does_not_hide_a_stack_write(self):
        with self.assertRaisesRegex(ValueError, "unreachable stack write"):
            stack.stack_frame(["100: jr a0", "104: sub sp, sp, a2", "108: ret"])

    def test_balanced_indirect_tail_call_stays_on_frontier(self):
        body = ["100: addi sp, sp, -16", "104: addi sp, sp, 16", "108: jr a0"]
        self.assertEqual(stack.stack_frame(body), 16)
        _, frontier = stack.direct_graph({"a": body}, {"a"})
        self.assertEqual(frontier[0]["kind"], "unresolved-transfer")

    def test_zero_link_jal_and_pc_relative_jr_are_internal_edges(self):
        for body in (["100: addi sp, sp, -16", "104: jal zero, 0x100 <f>"], [
            "100: addi sp, sp, -16", "104: auipc t1, 0x0", "108: jr -0x4(t1) <f>",
        ]):
            with self.subTest(body=body), self.assertRaisesRegex(ValueError, "stack-depth cycle"):
                stack.stack_frame(body)

    def test_branch_cannot_bypass_pc_relative_target_definition(self):
        with self.assertRaisesRegex(ValueError, "unresolved in-frame transfer"):
            stack.stack_frame([
                "100: addi sp, sp, -16", "104: beqz a0, 0x10c",
                "108: auipc t1, 0x1", "10c: jr 0x0(t1) <outside>",
            ])

    def test_branch_into_missing_instruction_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "unparsed instruction"):
            stack.stack_frame(["100: beqz a0, 0x102", "104: ret"])

    def test_epilogue_cannot_release_an_unallocated_frame(self):
        with self.assertRaisesRegex(ValueError, "outside supported entry-relative range"):
            stack.stack_frame([
                "100: beqz a0, 0x108", "104: addi sp, sp, -16",
                "108: addi sp, sp, 16", "10c: ret",
            ])


if __name__ == "__main__":
    unittest.main()
