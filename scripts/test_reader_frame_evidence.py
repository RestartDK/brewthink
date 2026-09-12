from collections import Counter
import importlib.util
from pathlib import Path
import sys
import unittest

import reader_frame_evidence as evidence

SPEC = importlib.util.spec_from_file_location("frame_test_stack", Path(__file__).with_name("check-reader-stack.py"))
STACK = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = STACK
SPEC.loader.exec_module(STACK)


def records(size=16, slot="Offset: [SP-4], Type: Variable, Align: 4, Size: 4"):
    return f"note: source.rs:1:0 prologepilog (analysis): {size} stack bytes in function 'reader'\n" + \
        "note: source.rs:1:0 stack-frame-layout (analysis):\nFunction: reader\n" + (slot + "\n" if slot else "") + "\n"


def machine(body):
    return ("# *** IR Dump Before RISC-V Assembly Printer (riscv-asm-printer) ***:\n"
            f"# Machine code for function reader: {evidence.MACHINE_PROPERTIES}\n"
            + body + "\n# End machine code for function reader.\n")


FRAME = {"size": 16, "slots": [(-4, "Variable", 4, 4)]}
BODY = ["bb.0:", "  $x2 = frame-setup ADDI $x2, -16", "  $x2 = frame-destroy ADDI $x2, 16", "  PseudoRET"]
ASSEMBLY = 'INLINEASM &"csrrci ${0}, mstatus, 8" [sideeffect] [attdialect], $0:[regdef-ec:GPRNoX0], def early-clobber renamable $x10'


class FrameRecordTests(unittest.TestCase):
    def test_explicit_zero_and_fixed_variable_slot(self):
        self.assertEqual(evidence.frame_records(records().splitlines(), {"reader"})["reader"], FRAME)
        self.assertEqual(evidence.frame_records(records(0, "").splitlines(), {"reader"})["reader"]["size"], 0)

    def test_missing_and_duplicate_records_are_not_zero(self):
        for text in ["", records().split("note: source.rs:1:0 stack-frame-layout")[0], records() * 2]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                evidence.frame_records(text.splitlines(), {"reader"})

    def test_reject_dynamic_scalable_and_overaligned_slots(self):
        for slot in ["Offset: [SP-4], Type: VariableSized, Align: 4, Size: 4",
                     "Offset: [SP-4], Type: Variable, Align: 4, Size: scalable 4",
                     "Offset: [SP-4], Type: Variable, Align: 32, Size: 4",
                     "Offset: [SP-32], Type: Variable, Align: 4, Size: 4"]:
            with self.subTest(slot=slot), self.assertRaises(ValueError):
                evidence.frame_records(records(slot=slot).splitlines(), {"reader"})

    def test_reject_missing_slot_size_and_malformed_record(self):
        for text in [records().replace("Size: 4", "Size: unknown"), records(size=15), records().replace("16 stack bytes", "-16 stack bytes")]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                evidence.frame_records(text.splitlines(), {"reader"})

    def test_terminal_phase_complete_unique_machine_record(self):
        text = machine("\n".join(BODY))
        parsed = evidence.machine_records(text.splitlines(), {"reader"})
        self.assertEqual(evidence.machine_frame(parsed["reader"], FRAME)["size"], 16)
        for invalid in [text * 2, text.replace("# End machine", "# Missing machine"),
                        text.replace("Before RISC-V Assembly Printer (riscv-asm-printer)", "After Prologue/Epilogue Insertion & Frame Finalization (prologepilog)"),
                        text.replace("NoVRegs", "HasVRegs")]:
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                evidence.machine_records(invalid.splitlines(), {"reader"})


class MachineFrameTests(unittest.TestCase):
    def test_frame_and_explicit_zero(self):
        proof = evidence.machine_frame(BODY, FRAME)
        self.assertEqual(proof["sp_writes"], Counter({-16: 1, 16: 1}))
        self.assertEqual(evidence.machine_frame(["bb.0:", "  PseudoRET"], {"size": 0})["size"], 0)

    def test_reject_nonconstant_absolute_untagged_and_aliased_sp(self):
        for replacement in ["  $x2 = frame-setup ADD $x2, $x10", "  $x2 = COPY $x8",
                            "  $x2 = ADDI $x2, -16", "  $sp = frame-setup ADDI $sp, -16",
                            "  renamable $x2 = frame-setup ADDI $x2, -16", "  $x2 = frame-setup ADDI $x2, -15"]:
            with self.subTest(replacement=replacement), self.assertRaises(ValueError):
                evidence.machine_frame([BODY[0], replacement, *BODY[2:]], FRAME)

    def test_allocating_cycle_and_unreachable_stack_write_are_rejected(self):
        cases = [
            ["bb.0:", "  successors: %bb.0(0x80000000)", BODY[1], "  PseudoBR %bb.0"],
            BODY + ["bb.1:", BODY[1], BODY[2], "  PseudoRET"],
        ]
        for body in cases:
            with self.subTest(body=body), self.assertRaises(ValueError):
                evidence.machine_frame(body, FRAME)

    def test_terminal_instruction_cannot_hide_an_undeclared_branch(self):
        cases = [
            ["bb.0:", "  BEQ $x10, $x11, %bb.999", "  PseudoRET"],
            ["Jump Tables:", "%jump-table.0: %bb.1", "bb.0:", "  successors: %bb.1(0x80000000)",
             "  BEQ $x10, $x11, %bb.999", "  PseudoBRIND renamable $x10", "bb.1:", "  PseudoRET"],
        ]
        for body in cases:
            with self.subTest(body=body), self.assertRaises(ValueError):
                evidence.machine_frame(body, {"size": 0})

    def test_unknown_instruction_and_nonstandard_helpers_are_rejected(self):
        for instruction in ["  STACK_SWITCH $x10", "  PseudoCALL target-flags(riscv-call) &__riscv_save_12, <regmask >"]:
            with self.subTest(instruction=instruction), self.assertRaises(ValueError):
                evidence.machine_frame([BODY[0], BODY[1], instruction, *BODY[2:]], FRAME)

    def test_ordinary_external_call_and_nonreturning_block(self):
        body = [BODY[0], BODY[1], "  PseudoCALL target-flags(riscv-call) &memcpy, <regmask >", *BODY[2:]]
        self.assertEqual(evidence.machine_frame(body, FRAME)["calls"], ["memcpy"])
        body = [BODY[0], BODY[1], "  PseudoCALL target-flags(riscv-call) @panic, <regmask >"]
        self.assertEqual(evidence.machine_frame(body, FRAME)["size"], 16)

    def test_csr_template_is_admitted_but_stack_memory_and_jumps_are_not(self):
        self.assertEqual(evidence.benign_assembly(ASSEMBLY), "csrrci ${0}, mstatus, 8")
        for template in ["sw x0, -32(sp)", "jr x10", "addi sp, sp, -16", "csrrci ${0}, mstatus, 8; sw x0, -32(sp)"]:
            with self.subTest(template=template), self.assertRaises(ValueError):
                evidence.benign_assembly(ASSEMBLY.replace("csrrci ${0}, mstatus, 8", template))

    def test_reserved_and_unknown_assembly_register_aliases_are_rejected(self):
        for register in ["$sp", "$x2", "$x2_w", "$fp", "$x8", "$x8_h", "$ra", "$s0", "$v8"]:
            with self.subTest(register=register), self.assertRaises(ValueError):
                evidence.benign_assembly(ASSEMBLY + ", implicit-def " + register)

    def test_local_dispatch_requires_closed_compiler_jump_table(self):
        body = ["Jump Tables:", "%jump-table.0: %bb.1 %bb.2", "bb.0:",
                "  successors: %bb.1(0x40000000), %bb.2(0x40000000)", BODY[1],
                "  PseudoBRIND renamable $x10", "bb.1:", BODY[2], "  PseudoRET",
                "bb.2:", BODY[2], "  PseudoRET"]
        self.assertEqual(evidence.machine_frame(body, FRAME)["indirect"], [("PseudoBRIND", "a0")])
        with self.assertRaises(ValueError):
            evidence.machine_frame(body[2:], FRAME)


class EmittedCorroborationTests(unittest.TestCase):
    def test_actual_sp_sites_and_compiler_frame_must_agree(self):
        body = [" 1000: addi sp, sp, -0x10", " 1002: addi sp, sp, 0x10", " 1004: ret"]
        proof = evidence.machine_frame(BODY, FRAME)
        self.assertEqual(evidence.corroborate(STACK, body, proof), set())
        for broken in [[" 1000: mv sp, a0", *body[1:]], [" 1000: cm.push sp", *body[1:]],
                       [" 1000: addi x2, x2, -0x20", *body[1:]]]:
            with self.subTest(broken=broken), self.assertRaises(ValueError):
                evidence.corroborate(STACK, broken, proof)
        with self.assertRaisesRegex(ValueError, "contradicts compiler size"):
            evidence.corroborate(STACK, body, {**proof, "size": 32})

    def test_known_computed_allocating_cycle_is_not_overridden(self):
        proof = {"size": 16, "sp_writes": Counter({-16: 1}), "indirect": [("PseudoBRIND", "a0")]}
        body = ["1000: addi sp, sp, -0x10", "1002: li a0, 0x1000", "1004: jr a0"]
        with self.assertRaises(ValueError):
            evidence.corroborate(STACK, body, proof, {0x1004: {0x1000}})

    def test_compiler_record_cannot_override_a_known_allocating_cycle(self):
        proof = {"size": 16, "sp_writes": Counter({-16: 1}), "indirect": []}
        with self.assertRaises(ValueError):
            evidence.corroborate(STACK, [" 1000: addi sp, sp, -0x10", " 1002: j 0x1000"], proof)


if __name__ == "__main__":
    unittest.main()
