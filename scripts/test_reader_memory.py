import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import tarfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("reader_memory", Path(__file__).with_name("check-reader-memory.py"))
memory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(memory)


class DecompressorSourceContractTests(unittest.TestCase):
    def test_version_change_requires_review(self):
        with self.assertRaisesRegex(ValueError, "version changed"):
            memory.verify_contract({"version": "0.9.2"})

    def test_same_version_with_changed_private_source_is_rejected(self):
        target = Path(os.environ.get("CARGO_TARGET_DIR", memory.ROOT / "target")) / "source-contract-tests"
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as directory:
            root = Path(directory)
            source = root / "state.rs"
            source.write_bytes(b"reviewed private fields")
            package = {"version": "0.9.1", "manifest_path": str(root / "Cargo.toml"), "id": "test", "source": None}
            expected = {"state.rs": memory.digest(source.read_bytes())}
            with patch.object(memory, "CONTRACT_SOURCES", expected):
                self.assertEqual(memory.verify_contract(package)["files_sha256"], expected)
                source.write_bytes(b"a new reference field")
                with self.assertRaisesRegex(ValueError, "source changed"):
                    memory.verify_contract(package)


class EvidenceBindingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.inputs = {"source": "current", "target_directory": str(self.root / "target")}
        self.generated = {"release/build/test/out/link.x": "generated-hash"}
        for name in memory.ARTIFACTS:
            (self.root / name).write_bytes(b"same-elf" if name.endswith(".elf") else b"compiler-records")
        self.bundle = {
            "schema": 2, "state": "linked", "inputs": self.inputs, "symbols": {"reader": {}},
            "generated_inputs": self.generated, "diagnostics": memory.DIAGNOSTICS,
            "command": memory.build_command(),
            "builds": [{"phase": phase, "command": memory.phase_command(phase, {"reader": {}}),
                        "clean": memory.clean_command(), "exit": 0} for phase in ("frames", "machine")],
            "artifacts": {name: memory.digest((self.root / name).read_bytes()) for name in memory.ARTIFACTS},
        }
        for name, value in [("build_environment", {}), ("cargo_metadata", {}),
                            ("stable_inputs", self.inputs), ("generated_inputs", self.generated)]:
            replacement = patch.object(memory, name, return_value=value)
            replacement.start()
            self.addCleanup(replacement.stop)
        self.save(self.bundle)

    def save(self, bundle):
        (self.root / "inputs.json").write_text(json.dumps(bundle))

    def validate(self):
        return memory.validate_bundle(self.root, self.root / "reader.elf")

    def test_complete_matching_bundle_is_accepted(self):
        self.assertEqual(self.validate()["symbols"], {"reader": {}})

    def test_partial_duplicate_or_malformed_metadata_is_rejected(self):
        for schema, state in [(1, "linked"), (2.0, "linked"), (True, "linked"), (2, "building")]:
            with self.subTest(schema=schema, state=state):
                self.save({**self.bundle, "schema": schema, "state": state})
                with self.assertRaises(ValueError):
                    self.validate()
        (self.root / "inputs.json").write_text('{"schema":2,"schema":2}')
        with self.assertRaisesRegex(ValueError, "duplicate"):
            self.validate()

    def test_each_required_artifact_is_bound(self):
        for name in memory.ARTIFACTS:
            with self.subTest(name=name):
                bundle = copy.deepcopy(self.bundle)
                del bundle["artifacts"][name]
                self.save(bundle)
                with self.assertRaises(ValueError):
                    self.validate()
        self.save(self.bundle)
        for name in memory.ARTIFACTS:
            with self.subTest(name=name):
                path = self.root / name
                original = path.read_bytes()
                path.write_bytes(b"stale-or-corrupt")
                with self.assertRaises(ValueError):
                    self.validate()
                path.write_bytes(original)

    def test_cross_elf_bundle_and_wrong_requested_image_are_rejected(self):
        changed = self.root / "frames.elf"
        changed.write_bytes(b"other-link")
        bundle = copy.deepcopy(self.bundle)
        bundle["artifacts"]["frames.elf"] = memory.digest(changed.read_bytes())
        self.save(bundle)
        with self.assertRaisesRegex(ValueError, "exact intended ELF"):
            self.validate()
        changed.write_bytes(b"same-elf")
        self.save(self.bundle)
        wrong = self.root / "wrong.elf"
        wrong.write_bytes(b"different-elf")
        with self.assertRaisesRegex(ValueError, "exact intended ELF"):
            memory.validate_bundle(self.root, wrong)

    def test_different_source_or_generated_inputs_are_rejected(self):
        with patch.object(memory, "stable_inputs", return_value={**self.inputs, "source": "changed"}):
            with self.assertRaisesRegex(ValueError, "inputs changed"):
                self.validate()
        with patch.object(memory, "generated_inputs", return_value={}):
            with self.assertRaisesRegex(ValueError, "generated build inputs changed"):
                self.validate()

    def completed(self, verdict="PASS_LIMITED", inputs=None):
        manifest_hash = memory.digest((self.root / "inputs.json").read_bytes())
        elf_hash = self.bundle["artifacts"]["reader.elf"]
        report = {"verdict": verdict, "whole_program_bound": False, "elf_sha256": elf_hash,
                  "evidence_inputs_sha256": inputs or manifest_hash}
        (self.root / "stack.json").write_text(json.dumps(report))
        marker = {"inputs_sha256": manifest_hash, "elf_sha256": elf_hash,
                  "stack_sha256": memory.digest((self.root / "stack.json").read_bytes())}
        (self.root / "verified.json").write_text(json.dumps(marker))

    def test_packaging_requires_completed_exact_passing_proof(self):
        with self.assertRaises(FileNotFoundError):
            memory.verify_proof(self.root, self.root / "reader.elf")
        self.completed()
        self.assertEqual(memory.verify_proof(self.root, self.root / "reader.elf")["state"], "linked")
        (self.root / "stack.json").write_text("changed report")
        with self.assertRaises(ValueError):
            memory.verify_proof(self.root, self.root / "reader.elf")
        for verdict, inputs in [("BLOCKED_BUDGET", None), ("PASS_LIMITED", "wrong-inputs")]:
            self.completed(verdict, inputs)
            with self.assertRaisesRegex(ValueError, "does not pass"):
                memory.verify_proof(self.root, self.root / "reader.elf")

    def test_missing_clean_failed_build_or_codegen_change_is_rejected(self):
        for mutation in ["clean", "exit", "boolean-exit", "codegen", "missing-phase"]:
            with self.subTest(mutation=mutation):
                bundle = copy.deepcopy(self.bundle)
                if mutation == "missing-phase":
                    bundle["builds"].pop()
                elif mutation == "codegen":
                    bundle["builds"][1]["command"] += ["-C", "opt-level=0"]
                elif mutation == "clean":
                    bundle["builds"][0]["clean"] = []
                else:
                    bundle["builds"][0]["exit"] = False if mutation == "boolean-exit" else 1
                self.save(bundle)
                with self.assertRaises(ValueError):
                    self.validate()


class RegistryBindingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        registry = Path(temporary.name) / "registry"
        self.root = registry / "src/index/sample-0.1.0"
        (self.root / "src").mkdir(parents=True)
        self.data = b"pub fn sample() {}"
        (self.root / "src/lib.rs").write_bytes(self.data)
        self.archive = registry / "cache/index/sample-0.1.0.crate"
        self.archive.parent.mkdir(parents=True)
        with tarfile.open(self.archive, "w:gz") as archive:
            member = tarfile.TarInfo("sample-0.1.0/src/lib.rs")
            member.size = len(self.data)
            archive.addfile(member, io.BytesIO(self.data))
        self.checksum = memory.digest(self.archive.read_bytes())
        self.package = {"name": "sample", "version": "0.1.0", "manifest_path": str(self.root / "Cargo.toml")}

    def test_normal_registry_cache_without_vendor_metadata_is_bound(self):
        self.assertFalse((self.root / ".cargo-checksum.json").exists())
        self.assertEqual(len(memory.registry_sources(self.package, self.checksum)), 64)

    def test_changed_archive_changed_source_and_extra_file_are_rejected(self):
        source = self.root / "src/lib.rs"
        source.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "source differs"):
            memory.registry_sources(self.package, self.checksum)
        source.write_bytes(self.data)
        extra = self.root / "new.rs"
        extra.write_text("extra source")
        with self.assertRaisesRegex(ValueError, "inventory changed"):
            memory.registry_sources(self.package, self.checksum)
        extra.unlink()
        self.archive.write_bytes(b"changed archive")
        with self.assertRaisesRegex(ValueError, "archive checksum"):
            memory.registry_sources(self.package, self.checksum)

    def test_vendor_checksums_must_match_lock_and_files(self):
        metadata = {"package": self.checksum, "files": {"src/lib.rs": memory.digest(self.data)}}
        (self.root / ".cargo-checksum.json").write_text(json.dumps(metadata))
        self.archive.unlink()
        self.assertEqual(len(memory.registry_sources(self.package, self.checksum)), 64)
        with self.assertRaisesRegex(ValueError, "Cargo.lock"):
            memory.registry_sources(self.package, "0" * 64)


class ProducerBoundaryTests(unittest.TestCase):
    def test_bootstrap_wrappers_and_codegen_overrides_are_rejected(self):
        for name in ["RUSTC_BOOTSTRAP", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTFLAGS",
                     "CARGO_ENCODED_RUSTFLAGS", "CARGO_PROFILE_RELEASE_OPT_LEVEL", "CARGO_TARGET_RISCV32IMC_UNKNOWN_NONE_ELF_RUSTFLAGS",
                     "CARGO_BUILD_RUSTC", "CARGO_BUILD_RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTFLAGS", "CARGO_INCLUDE"]:
            with self.subTest(name=name), patch.dict(os.environ, {name: "override"}, clear=True):
                with self.assertRaisesRegex(ValueError, "code-generation override"):
                    memory.build_environment()

    def test_cargo_configuration_cannot_select_an_unidentified_compiler(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".cargo").mkdir()
            configuration = root / ".cargo/config.toml"
            overrides = ['[build]\nrustc="other"', '[build]\nrustc-wrapper="other"',
                         '[build]\nrustc-workspace-wrapper="other"', '[env]\nRUSTC_BOOTSTRAP="1"',
                         '[env]\nCARGO_BUILD_RUSTC="other"',
                         '[env]\nRUSTUP_TOOLCHAIN={value="other-installed-toolchain", force=true}',
                         '[env]\nRUSTUP_HOME={value="other", force=true}',
                         '[env]\nPATH={value="other", force=true}',
                         '[env]\nCARGO_HOME={value="other", force=true}',
                         '[env]\nBREWTHINK_DIAGNOSTIC_STAGE={value="other", force=true}']
            for text in overrides:
                configuration.write_text(text)
                with self.subTest(text=text), patch.object(memory, "ROOT", root), \
                     patch.dict(os.environ, {"CARGO_HOME": str(root / "home")}, clear=True):
                    with self.assertRaises(ValueError):
                        memory.cargo_configuration()

    def test_cargo_home_must_not_resolve_against_another_working_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "target/cargo-home"
            home.mkdir(parents=True)
            (home / "config.toml").write_text('[build]\nrustc="other"\n')
            with patch.object(memory, "ROOT", root), patch.dict(os.environ, {"CARGO_HOME": "target/cargo-home"}, clear=True):
                with self.assertRaisesRegex(ValueError, "absolute"):
                    memory.cargo_configuration()

    def test_reviewed_logging_configuration_is_hashed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".cargo").mkdir()
            configuration = root / ".cargo/config.toml"
            configuration.write_text('[env]\nDEFMT_LOG={value="info", force=true}\n')
            with patch.object(memory, "ROOT", root), patch.dict(os.environ, {"CARGO_HOME": str(root / "home")}, clear=True):
                self.assertEqual(memory.cargo_configuration()[str(configuration)], memory.digest(configuration.read_bytes()))

    def test_cargo_includes_cannot_escape_configuration_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".cargo").mkdir()
            (root / ".cargo/config.toml").write_text('include = ["other.toml"]\n')
            (root / ".cargo/other.toml").write_text('[build]\nrustc = "other"\n')
            with patch.object(memory, "ROOT", root), patch.dict(os.environ, {"CARGO_HOME": str(root / "home")}, clear=True):
                with self.assertRaisesRegex(ValueError, "include"):
                    memory.cargo_configuration()

    def test_ci_fetches_locked_dependencies_before_offline_checks(self):
        workflow = (memory.ROOT / ".github/workflows/rust_ci.yml").read_text()
        firmware = workflow.split("  firmware:\n", 1)[1].split("  web-simulator:\n", 1)[0]
        fetch = firmware.index("- run: cargo fetch --locked")
        self.assertLess(fetch, firmware.index("- run: python3 -m unittest"))
        self.assertLess(fetch, firmware.index("- run: scripts/check-firmware.sh"))

    def test_stage_and_storage_are_not_silently_changed(self):
        for environment in [{"BREWTHINK_DIAGNOSTIC_STAGE": "grayscale-bench"}, {"BREWTHINK_PREVIOUS_FRAME_STORAGE": "host-ram"}]:
            with self.subTest(environment=environment), patch.dict(os.environ, environment, clear=True):
                with self.assertRaises(ValueError):
                    memory.build_environment()
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(memory.build_environment()["BREWTHINK_PREVIOUS_FRAME_STORAGE"], "controller-ram")

    def test_nonignored_output_is_rejected_before_build_inputs_are_read(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(memory, "ROOT", Path(directory)), \
             patch.object(memory.subprocess, "run") as run, patch.object(memory, "cargo_metadata") as metadata:
            run.return_value.returncode = 1
            with self.assertRaisesRegex(ValueError, "external or gitignored"):
                memory.produce(Path(directory) / "unignored")
            metadata.assert_not_called()

    def test_reused_evidence_directory_cannot_be_overwritten(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(memory, "build_environment", return_value={}), \
             patch.object(memory, "cargo_metadata", return_value={"packages": [{"name": "miniz_oxide"}]}), \
             patch.object(memory, "stable_inputs", return_value={}), patch.object(memory, "verify_contract", return_value={}):
            marker = Path(directory) / "keep"
            marker.write_text("existing evidence")
            with self.assertRaises(FileExistsError):
                memory.produce(Path(directory))
            self.assertEqual(marker.read_text(), "existing evidence")


if __name__ == "__main__":
    unittest.main()
