#!/usr/bin/env python3
"""Produce fresh, source-bound fixed-frame evidence for the actual release reader."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tomllib


ROOT = Path(__file__).resolve().parent.parent
TARGET = "riscv32imc-unknown-none-elf"
ARTIFACTS = {"frames.log", "machine.log", "frames.elf", "reader.elf"}
DIAGNOSTICS = {
    "frames": ["-C", "remark=prologepilog stack-frame-layout"],
    "machine": ["-C", "llvm-args=-print-before=riscv-asm-printer"],
}
COMPILER_OVERRIDES = {
    "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC_BOOTSTRAP",
    "CARGO_BUILD_RUSTC", "CARGO_BUILD_RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTFLAGS", "CARGO_INCLUDE",
}
BUILD_ENVIRONMENT = {
    "BREWTHINK_DIAGNOSTIC_STAGE", "BREWTHINK_PREVIOUS_FRAME_STORAGE",
    "BREWTHINK_DISPLAY_ROTATION", "BREWTHINK_X4_DRIVE_PROFILE", "BREWTHINK_DISPLAY_REFRESH",
    "CARGO_TARGET_DIR", "CARGO_BUILD_JOBS", "RUSTUP_TOOLCHAIN", "DEFMT_LOG",
}
CONTRACT_SOURCES = {
    "src/lib.rs": "87da790f8aeba6da7d9f9ef3e2b48a0d6ad785a0199666754f03b4d1aa3973dc",
    "src/inflate/core.rs": "41a52ea2145885701df41e77b4427aecfa0669971bbc7abb7c90bfe2cfe9fd74",
    "src/inflate/stream.rs": "e6dd9e06e927dbe5a59299db1bc96bf1c55cad8ef995fccec314ecf056622884",
    "src/inflate/mod.rs": "24a041eeb9404079395fd8ac5a51b8e2494fdf46a511681bb727e0b2b7017b57",
}


def output(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate evidence key: {key}")
        result[key] = value
    return result


def read_json(path):
    return json.loads(path.read_text(), object_pairs_hook=unique_pairs)


def source_inputs():
    paths = output("git", "ls-files", "--cached", "--others", "--exclude-standard", "-z").split("\0")
    hashes = {name: digest((ROOT / name).read_bytes()) if (ROOT / name).is_file() else None for name in sorted(set(paths)) if name}
    return {
        "head": output("git", "rev-parse", "HEAD"),
        "diff_sha256": digest(subprocess.check_output(["git", "diff", "HEAD", "--binary"], cwd=ROOT)),
        "files_sha256": hashes,
        "source_digest": digest(json.dumps(hashes, sort_keys=True).encode()),
    }


def verify_contract(package):
    if package["version"] != "0.9.1":
        raise ValueError("miniz_oxide version changed; review the zero-validity contract")
    root = Path(package["manifest_path"]).parent
    actual = {name: digest((root / name).read_bytes()) for name in CONTRACT_SOURCES}
    if actual != CONTRACT_SOURCES:
        raise ValueError("miniz_oxide source changed; review docs/reader-memory.md before updating contract hashes")
    return {"id": package["id"], "source": package["source"], "files_sha256": actual}


def compiler_overrides(names):
    return sorted(name for name in names if name in COMPILER_OVERRIDES or name.startswith("CARGO_PROFILE_") or
                  (name.startswith("CARGO_TARGET_") and name != "CARGO_TARGET_DIR"))


def build_environment():
    environment = os.environ.copy()
    present = compiler_overrides(environment)
    if present:
        raise ValueError(f"unreviewed reader code-generation override: {', '.join(present)}")
    for name, expected in {"BREWTHINK_DIAGNOSTIC_STAGE": "reader-app", "BREWTHINK_PREVIOUS_FRAME_STORAGE": "controller-ram"}.items():
        if environment.get(name, expected) != expected:
            raise ValueError(f"reader memory evidence requires {name}={expected}")
        environment[name] = expected
    cargo_configuration()
    environment.update(CARGO_NET_OFFLINE="true", CARGO_TERM_COLOR="never")
    return environment


def compiler_identity():
    rustc = output("rustc", "-vV")
    if "\nrelease: 1.97.1\n" not in rustc or not rustc.endswith("LLVM version: 22.1.6"):
        raise ValueError("fixed-frame parser requires rustc 1.97.1 / LLVM 22.1.6")
    return {"rustc": rustc, "cargo": output("cargo", "--version"), "sysroot": output("rustc", "--print", "sysroot")}


def cargo_metadata():
    return json.loads(output("cargo", "metadata", "--offline", "--locked", "--format-version=1", "--filter-platform", TARGET,
                             "--no-default-features", "--features", "device-reader"))


def registry_sources(package, checksum):
    root = Path(package["manifest_path"]).parent.resolve()
    vendor = root / ".cargo-checksum.json"
    if vendor.is_file():
        declaration = read_json(vendor)
        if declaration.get("package") != checksum:
            raise ValueError("vendored package checksum differs from Cargo.lock")
        declared = declaration["files"]
    else:
        filename = package["name"] + "-" + package["version"]
        archive = root.parent.parent.parent / "cache" / root.parent.name / (filename + ".crate")
        if digest(archive.read_bytes()) != checksum:
            raise ValueError("registry archive checksum differs from Cargo.lock")
        declared = {}
        with tarfile.open(archive) as source:
            for member in source:
                if member.isdir():
                    continue
                parts = Path(member.name).parts
                if not member.isfile() or len(parts) < 2 or parts[0] != filename or ".." in parts:
                    raise ValueError("unsupported registry archive member")
                name = str(Path(*parts[1:]))
                if name in declared:
                    raise ValueError("duplicate registry archive member")
                with source.extractfile(member) as stream:
                    declared[name] = digest(stream.read())
    if not isinstance(declared, dict) or not declared:
        raise ValueError("missing registry source checksums")
    for name, expected in declared.items():
        path = root / name
        if not path.resolve().is_relative_to(root) or digest(path.read_bytes()) != expected:
            raise ValueError(f"registry source differs from checksum: {package['name']}/{name}")
    actual = {str(path.relative_to(root)) for path in root.rglob("*") if path.is_file() and path.name not in {".cargo-ok", ".cargo-checksum.json"}}
    if actual != set(declared):
        raise ValueError(f"registry source file inventory changed: {package['name']}")
    return digest(json.dumps(declared, sort_keys=True).encode())


def dependency_inputs(metadata):
    locked = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    checksums = {(item["name"], item["version"], item.get("source")): item.get("checksum") for item in locked}
    dependencies = {}
    for package in metadata["packages"]:
        if Path(package["manifest_path"]).parent.resolve() == ROOT:
            continue
        if not (package["source"] or "").startswith("registry+"):
            raise ValueError("reader evidence requires reviewed registry dependencies or this workspace")
        checksum = checksums.get((package["name"], package["version"], package["source"]))
        if not isinstance(checksum, str) or len(checksum) != 64:
            raise ValueError("missing locked registry checksum")
        dependencies[package["id"]] = {"source": package["source"], "archive_sha256": checksum,
                                        "files_digest": registry_sources(package, checksum)}
    return dependencies


def generated_inputs(target):
    files = {}
    for directory in (target / "release/build", target / TARGET / "release/build"):
        if not directory.exists():
            continue
        for package in sorted(directory.iterdir()):
            for path in sorted((package / "out").rglob("*")):
                if path.is_file():
                    files[str(path.relative_to(target))] = digest(path.read_bytes())
            path = package / "output"
            if path.is_file():
                files[str(path.relative_to(target))] = digest(path.read_bytes())
    if not files:
        raise ValueError("missing generated build/linker inputs")
    return files


def cargo_configuration():
    home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
    if not home.is_absolute():
        raise ValueError("CARGO_HOME must be absolute for reader evidence")
    directories = {home, *(path / ".cargo" for path in [ROOT, *ROOT.parents])}
    hashes = {}
    for directory in sorted(directories):
        for path in (directory / "config", directory / "config.toml"):
            if not path.is_file():
                continue
            data = path.read_bytes()
            configuration = tomllib.loads(data.decode())
            if "include" in configuration:
                raise ValueError(f"Cargo configuration includes are unsupported by reader evidence: {path}")
            build = configuration.get("build", {})
            if set(build) & {"rustc", "rustc-wrapper", "rustc-workspace-wrapper"}:
                raise ValueError(f"Cargo configuration selects an unreviewed compiler or override: {path}")
            if set(configuration.get("env", {})) - {"DEFMT_LOG"}:
                raise ValueError(f"Cargo environment configuration only supports DEFMT_LOG for reader evidence: {path}")
            hashes[str(path)] = digest(data)
    return hashes


def stable_inputs(environment, metadata):
    return {
        "source": source_inputs(), "compiler": compiler_identity(), "cwd": str(ROOT),
        "environment": {name: environment[name] for name in sorted(BUILD_ENVIRONMENT) if name in environment},
        "cargo_configuration": cargo_configuration(),
        "metadata_sha256": digest(json.dumps(metadata, sort_keys=True).encode()),
        "dependencies": dependency_inputs(metadata), "target_directory": metadata["target_directory"],
    }


def load_stack():
    spec = importlib.util.spec_from_file_location("reader_stack", ROOT / "scripts/check-reader-stack.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def inventory(elf):
    stack = load_stack()
    disassembly = output("llvm-objdump", "-d", "--demangle", "--no-show-raw-insn", str(elf))
    code = elf.read_bytes()
    functions = stack.functions_from(disassembly, stack.function_extents(code))
    return stack.evidence.symbol_inventory(stack, code, functions,
                                           output("llvm-nm", "--defined-only", "--print-size", str(elf)))


def build_command():
    return ["cargo", "rustc", "--offline", "--locked", "--release", "--target", TARGET, "--bin", "brewthink",
            "--no-default-features", "--features", "device-reader", "--", "-D", "warnings"]


def clean_command():
    return ["cargo", "clean", "--release", "--target", TARGET, "--package", "brewthink"]


def phase_command(phase, symbols):
    command = build_command() + DIAGNOSTICS[phase]
    if phase == "machine":
        command += ["-C", "llvm-args=-filter-print-funcs=" + ",".join(sorted(symbols))]
    return command


def validate_bundle(directory, elf):
    directory, elf = directory.resolve(), elf.resolve()
    bundle = read_json(directory / "inputs.json")
    if not isinstance(bundle, dict) or type(bundle.get("schema")) is not int or bundle.get("schema") != 2 or bundle.get("state") != "linked":
        raise ValueError("missing complete fresh two-link evidence")
    expected_builds = [{"phase": phase, "command": phase_command(phase, bundle["symbols"]),
                        "clean": clean_command(), "exit": 0} for phase in ("frames", "machine")]
    if (bundle.get("command") != build_command() or bundle.get("builds") != expected_builds or
            any(type(build["exit"]) is not int for build in bundle["builds"])):
        raise ValueError("incomplete or changed diagnostic-only producer commands")
    if set(bundle.get("artifacts", {})) != ARTIFACTS:
        raise ValueError("missing compiler evidence artifact")
    for name, expected in bundle["artifacts"].items():
        path = directory / name
        if path.is_symlink() or digest(path.read_bytes()) != expected:
            raise ValueError(f"changed compiler evidence artifact: {name}")
    if bundle["artifacts"]["frames.elf"] != bundle["artifacts"]["reader.elf"] or digest(elf.read_bytes()) != bundle["artifacts"]["reader.elf"]:
        raise ValueError("compiler diagnostics do not describe the exact intended ELF")
    environment = build_environment()
    if stable_inputs(environment, cargo_metadata()) != bundle["inputs"]:
        raise ValueError("source/compiler/dependency/configuration inputs changed since the reader links")
    if generated_inputs(Path(bundle["inputs"]["target_directory"])) != bundle["generated_inputs"]:
        raise ValueError("generated build inputs changed since the reader links")
    if bundle.get("diagnostics") != DIAGNOSTICS:
        raise ValueError("unrecognized compiler diagnostic producer")
    return bundle


def verify_proof(directory, elf):
    bundle = validate_bundle(directory, elf)
    expected = {"inputs_sha256": digest((directory / "inputs.json").read_bytes()),
                "stack_sha256": digest((directory / "stack.json").read_bytes()),
                "elf_sha256": bundle["artifacts"]["reader.elf"]}
    if read_json(directory / "verified.json") != expected:
        raise ValueError("missing or changed completed reader proof")
    report = read_json(directory / "stack.json")
    if (report.get("verdict") != "PASS_LIMITED" or report.get("whole_program_bound") is not False or
            report.get("elf_sha256") != expected["elf_sha256"] or
            report.get("evidence_inputs_sha256") != expected["inputs_sha256"]):
        raise ValueError("reader proof does not pass for this exact ELF and input manifest")
    return bundle


def check_output_directory(directory):
    if directory.exists():
        raise FileExistsError(f"refusing to overwrite reader evidence: {directory}")
    if directory.is_relative_to(ROOT):
        result = subprocess.run(["git", "check-ignore", "--quiet", "--", str(directory)], cwd=ROOT)
        if result.returncode != 0:
            raise ValueError("reader evidence directory must be external or gitignored before building")


def produce(directory):
    check_output_directory(directory)
    environment = build_environment()
    metadata = cargo_metadata()
    inputs = stable_inputs(environment, metadata)
    packages = [package for package in metadata["packages"] if package["name"] == "miniz_oxide"]
    if len(packages) != 1:
        raise ValueError("expected one resolved miniz_oxide dependency")
    contract = verify_contract(packages[0])
    directory.mkdir(parents=True, exist_ok=False)
    target = Path(metadata["target_directory"])
    command = build_command()
    bundle = {"schema": 2, "state": "building", "inputs": inputs, "diagnostics": DIAGNOSTICS,
              "command": command, "decompressor_contract": contract, "builds": []}
    manifest = directory / "inputs.json"
    print(f"reader-memory source_digest={inputs['source']['source_digest']}", flush=True)
    for phase in ("frames", "machine"):
        arguments = phase_command(phase, bundle.get("symbols", {}))
        clean = clean_command()
        with (directory / (phase + "-clean.log")).open("w") as log:
            subprocess.run(clean, cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT, check=True)
        manifest.write_text(json.dumps(bundle, indent=2) + "\n")
        with (directory / (phase + ".log")).open("w") as log:
            result = subprocess.run(arguments, cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT)
        bundle["builds"].append({"phase": phase, "command": arguments, "clean": clean, "exit": result.returncode})
        manifest.write_text(json.dumps(bundle, indent=2) + "\n")
        if result.returncode:
            raise ValueError(f"reader {phase} link failed; see {directory / (phase + '.log')}")
        if stable_inputs(environment, cargo_metadata()) != inputs:
            raise ValueError("reader source/compiler/dependencies changed during the links")
        output_elf = directory / ("frames.elf" if phase == "frames" else "reader.elf")
        output_elf.write_bytes((target / TARGET / "release/brewthink").read_bytes())
        output_elf.chmod(0o444)
        generated = generated_inputs(target)
        if phase == "frames":
            bundle["symbols"] = inventory(output_elf)
            bundle["generated_inputs"] = generated
        elif generated != bundle["generated_inputs"] or output_elf.read_bytes() != (directory / "frames.elf").read_bytes():
            raise ValueError("diagnostic links changed generated inputs or intended firmware bytes")
    bundle["artifacts"] = {name: digest((directory / name).read_bytes()) for name in sorted(ARTIFACTS)}
    bundle["state"] = "linked"
    manifest.write_text(json.dumps(bundle, indent=2) + "\n")
    validate_bundle(directory, directory / "reader.elf")
    subprocess.run([sys.executable, str(ROOT / "scripts/check-reader-stack.py"), str(directory / "reader.elf"),
                    "--evidence", str(directory), "--report", str(directory / "stack.json")], cwd=ROOT, env=environment, check=True)
    validate_bundle(directory, directory / "reader.elf")
    completion = {"inputs_sha256": digest(manifest.read_bytes()), "stack_sha256": digest((directory / "stack.json").read_bytes()),
                  "elf_sha256": bundle["artifacts"]["reader.elf"]}
    (directory / "verified.json").write_text(json.dumps(completion, indent=2) + "\n")
    verify_proof(directory, directory / "reader.elf")
    print(f"reader-memory evidence={directory}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report_dir", type=Path, help="new directory for production, completed bundle for verification")
    parser.add_argument("--verify-elf", type=Path, help="verify a completed passing bundle without rebuilding")
    arguments = parser.parse_args()
    if arguments.verify_elf is None:
        produce(arguments.report_dir.resolve())
    else:
        verify_proof(arguments.report_dir.resolve(), arguments.verify_elf.resolve())
        print("reader-memory verified exact completed PASS_LIMITED artifact; whole_program_bound=false")


if __name__ == "__main__":
    main()
