use crate::{machine, re, stack};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap as Map, BTreeSet as Set},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub const TARGET: &str = "riscv32imc-unknown-none-elf";
pub const ARTIFACTS: &[&str] = &["frames.log", "machine.log", "frames.elf", "reader.elf"];
pub type Environment = Map<String, String>;
pub type Hashes = Map<String, String>;
const BUILD_ENVIRONMENT: &[&str] = &[
    "BREWTHINK_DIAGNOSTIC_STAGE",
    "BREWTHINK_PREVIOUS_FRAME_STORAGE",
    "BREWTHINK_DISPLAY_ROTATION",
    "BREWTHINK_X4_DRIVE_PROFILE",
    "BREWTHINK_DISPLAY_REFRESH",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_JOBS",
    "RUSTUP_TOOLCHAIN",
    "DEFMT_LOG",
];
const OVERRIDES: &[&str] = &[
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTC",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "RUSTC_BOOTSTRAP",
    "CARGO_BUILD_RUSTC",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTFLAGS",
    "CARGO_INCLUDE",
];
pub fn digest(data: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn hash(path: impl AsRef<Path>) -> Result<String> {
    Ok(digest(fs::read(path)?))
}
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(m) => json!(
            m.iter()
                .map(|(k, v)| (k, canonical(v)))
                .collect::<Map<_, _>>()
        ),
        Value::Array(v) => json!(v.iter().map(canonical).collect::<Vec<_>>()),
        _ => value.clone(),
    }
}
fn json_hash(value: &Value) -> String {
    digest(serde_json::to_vec(&canonical(value)).unwrap())
}
pub fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    let mut file = fs::File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

struct UniqueJson(Value);
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueJson(json!(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueJson(json!(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueJson(json!(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueJson(json!(v)))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueJson(json!(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut v = vec![];
                while let Some(UniqueJson(item)) = seq.next_element()? {
                    v.push(item)
                }
                Ok(UniqueJson(json!(v)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, UniqueJson(value))) = map.next_entry::<String, UniqueJson>()? {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate evidence key: {key}"
                        )));
                    }
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
pub fn read_json(path: impl AsRef<Path>) -> Result<Value> {
    Ok(serde_json::from_slice::<UniqueJson>(&fs::read(path)?)?.0)
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub address: u32,
    pub size: u32,
    pub code_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Build {
    pub phase: String,
    pub command: Vec<String>,
    pub clean: Vec<String>,
    pub exit: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub schema: u32,
    pub state: String,
    pub inputs: Value,
    pub symbols: Map<String, Symbol>,
    pub generated_inputs: Hashes,
    pub diagnostics: Map<String, Vec<String>>,
    pub command: Vec<String>,
    pub builds: Vec<Build>,
    pub artifacts: Hashes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decompressor_contract: Option<Value>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub manifest_path: PathBuf,
    #[serde(default)]
    pub id: String,
    pub source: Option<String>,
}
#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    target_directory: PathBuf,
}
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}
pub fn diagnostics() -> Map<String, Vec<String>> {
    Map::from([
        (
            "frames".into(),
            strings(&["-C", "remark=prologepilog stack-frame-layout"]),
        ),
        (
            "machine".into(),
            strings(&["-C", "llvm-args=-print-before=riscv-asm-printer"]),
        ),
    ])
}
pub fn build_command() -> Vec<String> {
    strings(&[
        "cargo",
        "rustc",
        "--offline",
        "--locked",
        "--release",
        "--target",
        TARGET,
        "--bin",
        "brewthink",
        "--no-default-features",
        "--features",
        "device-reader",
        "--",
        "-D",
        "warnings",
    ])
}
pub fn clean_command() -> Vec<String> {
    strings(&[
        "cargo",
        "clean",
        "--release",
        "--target",
        TARGET,
        "--package",
        "brewthink",
    ])
}
pub fn phase_command(phase: &str, symbols: &Map<String, Symbol>) -> Vec<String> {
    let mut args = build_command();
    args.extend(diagnostics()[phase].clone());
    if phase == "machine" {
        args.extend([
            "-C".into(),
            format!(
                "llvm-args=-filter-print-funcs={}",
                symbols.keys().cloned().collect::<Vec<_>>().join(",")
            ),
        ]);
    }
    args
}
pub fn compiler_overrides(environment: &Environment) -> Vec<String> {
    environment
        .keys()
        .filter(|n| {
            OVERRIDES.contains(&n.as_str())
                || n.starts_with("CARGO_PROFILE_")
                || (n.starts_with("CARGO_TARGET_") && *n != "CARGO_TARGET_DIR")
        })
        .cloned()
        .collect()
}
pub fn cargo_configuration(root: &Path, environment: &Environment) -> Result<Hashes> {
    let home = environment
        .get("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| environment.get("HOME").map(|v| Path::new(v).join(".cargo")))
        .context("missing Cargo home")?;
    ensure!(
        home.is_absolute(),
        "CARGO_HOME must be absolute for reader evidence"
    );
    let directories: Set<_> = root
        .ancestors()
        .map(|p| p.join(".cargo"))
        .chain([home])
        .collect();
    let mut hashes = Hashes::new();
    for dir in directories {
        for name in ["config", "config.toml"] {
            let path = dir.join(name);
            if !path.is_file() {
                continue;
            }
            let data = fs::read(&path)?;
            let config: toml::Value = toml::from_str(std::str::from_utf8(&data)?)?;
            ensure!(
                config.get("include").is_none(),
                "Cargo configuration includes are unsupported by reader evidence: {}",
                path.display()
            );
            ensure!(
                ["rustc", "rustc-wrapper", "rustc-workspace-wrapper"]
                    .iter()
                    .all(|k| config.get("build").and_then(|b| b.get(*k)).is_none()),
                "Cargo configuration selects an unreviewed compiler or override: {}",
                path.display()
            );
            ensure!(
                config
                    .get("env")
                    .and_then(toml::Value::as_table)
                    .is_none_or(|e| e.keys().all(|k| k == "DEFMT_LOG")),
                "Cargo environment configuration only supports DEFMT_LOG for reader evidence: {}",
                path.display()
            );
            hashes.insert(path.to_string_lossy().into(), digest(data));
        }
    }
    Ok(hashes)
}
pub fn build_environment(root: &Path, mut environment: Environment) -> Result<Environment> {
    let present = compiler_overrides(&environment);
    ensure!(
        present.is_empty(),
        "unreviewed reader code-generation override: {}",
        present.join(", ")
    );
    for (name, expected) in [
        ("BREWTHINK_DIAGNOSTIC_STAGE", "reader-app"),
        ("BREWTHINK_PREVIOUS_FRAME_STORAGE", "controller-ram"),
    ] {
        ensure!(
            environment.get(name).is_none_or(|v| v == expected),
            "reader memory evidence requires {name}={expected}"
        );
        environment.insert(name.into(), expected.into());
    }
    cargo_configuration(root, &environment)?;
    environment.insert("CARGO_NET_OFFLINE".into(), "true".into());
    environment.insert("CARGO_TERM_COLOR".into(), "never".into());
    Ok(environment)
}
pub fn verify_contract(package: &Package, sources: &Hashes) -> Result<Value> {
    ensure!(
        package.version == "0.9.1",
        "miniz_oxide version changed; review the zero-validity contract"
    );
    let root = package
        .manifest_path
        .parent()
        .context("missing dependency directory")?;
    let actual: Hashes = sources
        .keys()
        .map(|n| Ok((n.clone(), hash(root.join(n))?)))
        .collect::<Result<_>>()?;
    ensure!(
        actual == *sources,
        "miniz_oxide source changed; review docs/reader-memory.md before updating contract hashes"
    );
    Ok(json!({"id":package.id,"source":package.source,"files_sha256":actual}))
}
fn files(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut result = vec![];
    if !directory.exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            result.extend(files(&path)?);
        } else if path.is_file() {
            result.push(path);
        }
    }
    result.sort();
    Ok(result)
}
pub fn registry_sources(package: &Package, checksum: &str) -> Result<String> {
    let root = package
        .manifest_path
        .parent()
        .context("missing dependency directory")?
        .canonicalize()?;
    let vendor = root.join(".cargo-checksum.json");
    let declared: Hashes = if vendor.is_file() {
        let declaration = read_json(vendor)?;
        ensure!(
            declaration["package"] == checksum,
            "vendored package checksum differs from Cargo.lock"
        );
        serde_json::from_value(declaration["files"].clone())?
    } else {
        let filename = format!("{}-{}", package.name, package.version);
        let index = root.parent().context("missing registry index")?;
        let registry = index
            .parent()
            .and_then(Path::parent)
            .context("missing registry root")?;
        let archive = registry
            .join("cache")
            .join(index.file_name().unwrap())
            .join(format!("{filename}.crate"));
        let data = fs::read(archive)?;
        ensure!(
            digest(&data) == checksum,
            "registry archive checksum differs from Cargo.lock"
        );
        let reader: Box<dyn Read> = if data.starts_with(&[0x1f, 0x8b]) {
            Box::new(flate2::read::GzDecoder::new(std::io::Cursor::new(data)))
        } else {
            Box::new(std::io::Cursor::new(data))
        };
        let mut archive = tar::Archive::new(reader);
        let mut values = Hashes::new();
        for member in archive.entries()? {
            let mut member = member?;
            if member.header().entry_type().is_dir() {
                continue;
            }
            let path = member.path()?.into_owned();
            let parts: Vec<_> = path.components().collect();
            ensure!(
                member.header().entry_type().is_file()
                    && parts.len() >= 2
                    && parts
                        .iter()
                        .all(|p| matches!(p, std::path::Component::Normal(_)))
                    && parts[0].as_os_str() == filename.as_str(),
                "unsupported registry archive member"
            );
            let name = parts[1..]
                .iter()
                .collect::<PathBuf>()
                .to_string_lossy()
                .into_owned();
            let mut data = vec![];
            member.read_to_end(&mut data)?;
            ensure!(
                values.insert(name, digest(data)).is_none(),
                "duplicate registry archive member"
            );
        }
        values
    };
    ensure!(!declared.is_empty(), "missing registry source checksums");
    for (name, expected) in &declared {
        let path = root.join(name);
        ensure!(
            path.canonicalize()?.starts_with(&root) && hash(&path)? == *expected,
            "registry source differs from checksum: {}/{name}",
            package.name
        );
    }
    let actual: Set<_> = files(&root)?
        .iter()
        .filter(|p| {
            !matches!(
                p.file_name().and_then(|n| n.to_str()),
                Some(".cargo-ok" | ".cargo-checksum.json")
            )
        })
        .map(|p| {
            p.strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    ensure!(
        actual == declared.keys().cloned().collect(),
        "registry source file inventory changed: {}",
        package.name
    );
    Ok(json_hash(&json!(declared)))
}
pub fn generated_inputs(target: &Path) -> Result<Hashes> {
    let mut result = Hashes::new();
    for directory in [
        target.join("release/build"),
        target.join(TARGET).join("release/build"),
    ] {
        if !directory.exists() {
            continue;
        }
        for package in fs::read_dir(directory)? {
            let path = package?.path();
            let mut generated = files(&path.join("out"))?;
            if path.join("output").is_file() {
                generated.push(path.join("output"));
            }
            for path in generated {
                result.insert(
                    path.strip_prefix(target)?.to_string_lossy().into(),
                    hash(&path)?,
                );
            }
        }
    }
    ensure!(!result.is_empty(), "missing generated build/linker inputs");
    Ok(result)
}

pub fn validate_artifacts(directory: &Path, elf: &Path) -> Result<Bundle> {
    let bundle: Bundle = serde_json::from_value(read_json(directory.join("inputs.json"))?)
        .context("missing complete fresh two-link evidence")?;
    ensure!(
        bundle.schema == 2 && bundle.state == "linked",
        "missing complete fresh two-link evidence"
    );
    let expected: Vec<_> = ["frames", "machine"]
        .iter()
        .map(|p| Build {
            phase: (*p).into(),
            command: phase_command(p, &bundle.symbols),
            clean: clean_command(),
            exit: 0,
        })
        .collect();
    ensure!(
        bundle.command == build_command() && bundle.builds == expected,
        "incomplete or changed diagnostic-only producer commands"
    );
    ensure!(
        bundle
            .artifacts
            .keys()
            .map(String::as_str)
            .collect::<Set<_>>()
            == ARTIFACTS.iter().copied().collect(),
        "missing compiler evidence artifact"
    );
    for (name, expected) in &bundle.artifacts {
        let path = directory.join(name);
        ensure!(
            !path.is_symlink() && hash(path)? == *expected,
            "changed compiler evidence artifact: {name}"
        );
    }
    ensure!(
        bundle.artifacts["frames.elf"] == bundle.artifacts["reader.elf"]
            && hash(elf)? == bundle.artifacts["reader.elf"],
        "compiler diagnostics do not describe the exact intended ELF"
    );
    ensure!(
        bundle.diagnostics == diagnostics(),
        "unrecognized compiler diagnostic producer"
    );
    Ok(bundle)
}
pub fn validate_inputs(bundle: &Bundle, current: &Value, generated: &Hashes) -> Result<()> {
    ensure!(
        bundle.inputs == *current,
        "source/compiler/dependency/configuration inputs changed since the reader links"
    );
    ensure!(
        bundle.generated_inputs == *generated,
        "generated build inputs changed since the reader links"
    );
    Ok(())
}
pub fn verify_completion(directory: &Path, bundle: &Bundle) -> Result<()> {
    let expected = json!({"inputs_sha256":hash(directory.join("inputs.json"))?,"stack_sha256":hash(directory.join("stack.json"))?,"elf_sha256":bundle.artifacts["reader.elf"]});
    ensure!(
        read_json(directory.join("verified.json"))? == expected,
        "missing or changed completed reader proof"
    );
    let report = read_json(directory.join("stack.json"))?;
    ensure!(
        report["verdict"] == "PASS_LIMITED"
            && report["whole_program_bound"] == false
            && report["elf_sha256"] == expected["elf_sha256"]
            && report["evidence_inputs_sha256"] == expected["inputs_sha256"],
        "reader proof does not pass for this exact ELF and input manifest"
    );
    Ok(())
}

pub struct ReaderBuild {
    pub root: PathBuf,
    pub environment: Environment,
}
impl ReaderBuild {
    fn command(&self, args: &[String]) -> Command {
        let mut command = Command::new(&args[0]);
        command
            .args(&args[1..])
            .current_dir(&self.root)
            .env_clear()
            .envs(&self.environment);
        command
    }
    fn output_bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        let result = self.command(&strings(args)).output()?;
        ensure!(
            result.status.success(),
            "{} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(result.stdout)
    }
    fn output(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8(self.output_bytes(args)?)?.trim().into())
    }
    fn metadata(&self) -> Result<Value> {
        Ok(serde_json::from_str(&self.output(&[
            "cargo",
            "metadata",
            "--offline",
            "--locked",
            "--format-version=1",
            "--filter-platform",
            TARGET,
            "--no-default-features",
            "--features",
            "device-reader",
        ])?)?)
    }
    fn stable_inputs(&self, metadata: &Value) -> Result<Value> {
        let paths = self.output(&[
            "git",
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])?;
        let mut hashes = Map::new();
        for name in paths.split('\0').filter(|s| !s.is_empty()) {
            let path = self.root.join(name);
            hashes.insert(
                name,
                if path.is_file() {
                    Some(hash(path)?)
                } else {
                    None
                },
            );
        }
        let source = json!({"head":self.output(&["git","rev-parse","HEAD"])?,"diff_sha256":digest(self.output_bytes(&["git","diff","HEAD","--binary"])?),"files_sha256":hashes,"source_digest":json_hash(&json!(hashes))});
        let rustc = self.output(&["rustc", "-vV"])?;
        ensure!(
            rustc.contains("\nrelease: 1.97.1\n") && rustc.ends_with("LLVM version: 22.1.6"),
            "fixed-frame parser requires rustc 1.97.1 / LLVM 22.1.6"
        );
        let compiler = json!({"rustc":rustc,"cargo":self.output(&["cargo","--version"])?,"sysroot":self.output(&["rustc","--print","sysroot"])?});
        let parsed: Metadata = serde_json::from_value(metadata.clone())?;
        let lock: toml::Value = toml::from_str(&fs::read_to_string(self.root.join("Cargo.lock"))?)?;
        let locked = lock
            .get("package")
            .and_then(toml::Value::as_array)
            .context("missing locked dependencies")?;
        let mut dependencies = Map::new();
        for package in &parsed.packages {
            if package
                .manifest_path
                .parent()
                .context("missing manifest directory")?
                .canonicalize()?
                == self.root
            {
                continue;
            }
            ensure!(
                package
                    .source
                    .as_ref()
                    .is_some_and(|s| s.starts_with("registry+")),
                "reader evidence requires reviewed registry dependencies or this workspace"
            );
            let checksum = locked
                .iter()
                .find(|p| {
                    p.get("name").and_then(toml::Value::as_str) == Some(&package.name)
                        && p.get("version").and_then(toml::Value::as_str) == Some(&package.version)
                        && p.get("source").and_then(toml::Value::as_str)
                            == package.source.as_deref()
                })
                .and_then(|p| p.get("checksum"))
                .and_then(toml::Value::as_str)
                .filter(|s| s.len() == 64)
                .context("missing locked registry checksum")?;
            dependencies.insert(&package.id,json!({"source":package.source,"archive_sha256":checksum,"files_digest":registry_sources(package,checksum)?}));
        }
        Ok(
            json!({"source":source,"compiler":compiler,"cwd":self.root,"environment":self.environment.iter().filter(|(n,_)|BUILD_ENVIRONMENT.contains(&n.as_str())).collect::<Map<_,_>>(),"cargo_configuration":cargo_configuration(&self.root,&self.environment)?,"metadata_sha256":json_hash(metadata),"dependencies":dependencies,"target_directory":parsed.target_directory}),
        )
    }
    pub fn validate_bundle(&self, directory: &Path, elf: &Path) -> Result<Bundle> {
        let bundle = validate_artifacts(directory, elf)?;
        let current = self.stable_inputs(&self.metadata()?)?;
        let target = bundle.inputs["target_directory"]
            .as_str()
            .context("missing target directory")?;
        validate_inputs(&bundle, &current, &generated_inputs(Path::new(target))?)?;
        Ok(bundle)
    }
    pub fn verify_proof(&self, directory: &Path, elf: &Path) -> Result<Bundle> {
        let bundle = self.validate_bundle(directory, elf)?;
        verify_completion(directory, &bundle)?;
        Ok(bundle)
    }
    pub fn check_output_directory(&self, directory: &Path) -> Result<()> {
        ensure!(
            !directory.exists(),
            "refusing to overwrite reader evidence: {}",
            directory.display()
        );
        if directory.starts_with(&self.root) {
            ensure!(
                self.command(&strings(&[
                    "git",
                    "check-ignore",
                    "--quiet",
                    "--",
                    directory.to_str().context("non-UTF8 evidence path")?
                ]))
                .status()?
                .success(),
                "reader evidence directory must be external or gitignored before building"
            );
        }
        Ok(())
    }
    fn disassemble(&self, elf: &Path) -> Result<String> {
        self.output(&[
            "llvm-objdump",
            "-d",
            "--demangle",
            "--no-show-raw-insn",
            elf.to_str().context("non-UTF8 ELF path")?,
        ])
    }
    fn inventory(&self, elf: &Path, functions: &stack::Functions) -> Result<Map<String, Symbol>> {
        symbol_inventory(
            &fs::read(elf)?,
            functions,
            &self.output(&[
                "llvm-nm",
                "--defined-only",
                "--print-size",
                elf.to_str().context("non-UTF8 ELF path")?,
            ])?,
        )
    }
    pub fn analyze(
        &self,
        elf: &Path,
        directory: &Path,
        report_path: Option<&Path>,
        complete: bool,
    ) -> Result<Value> {
        let disassembly = self.disassemble(elf)?;
        let sizes = self.output(&[
            "llvm-size",
            "-A",
            elf.to_str().context("non-UTF8 ELF path")?,
        ])?;
        let sizes: Vec<_> = re(r"(?m)^\.stack\s+(\d+)")
            .captures_iter(&sizes)
            .map(|m| m[1].parse::<u64>())
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            sizes.len() == 1 && sizes[0] > 0,
            "ELF must have exactly one nonempty .stack section"
        );
        let bundle = self.validate_bundle(directory, elf)?;
        let code = fs::read(elf)?;
        let extents = stack::function_extents(&code)?;
        let functions = stack::functions_from(&disassembly, &extents)?;
        let inventory = self.inventory(elf, &functions)?;
        ensure!(
            inventory == bundle.symbols,
            "selected raw symbols/extents differ from linked evidence"
        );
        let selected = inventory.keys().cloned().collect();
        let lines = |name: &str| -> Result<Vec<String>> {
            Ok(fs::read_to_string(directory.join(name))?
                .lines()
                .map(String::from)
                .collect())
        };
        let records = machine::frame_records(&lines("frames.log")?, &selected)?;
        let machines = machine::machine_records(&lines("machine.log")?, &selected)?;
        let mut compiler = Map::new();
        for (raw, symbol) in &inventory {
            let mut proof = machine::machine_frame(&machines[raw], &records[raw])?;
            proof.callees = proof
                .calls
                .iter()
                .filter_map(|c| inventory.get(c).map(|s| s.name.clone()))
                .collect();
            compiler.insert(symbol.name.clone(), proof);
        }
        let mut report = stack::analyze(
            &disassembly,
            sizes[0],
            Some(&stack::readonly_sections(&code)?),
            Some(&compiler),
            &extents,
        )?;
        report["compiler_frames"] = json!(
            compiler
                .iter()
                .map(|(name, p)| {
                    let mut value = json!(p);
                    value.as_object_mut().unwrap().remove("sp_writes");
                    value["callees"] = json!(p.callees);
                    (name, value)
                })
                .collect::<Map<_, _>>()
        );
        report["evidence_inputs_sha256"] = json!(hash(directory.join("inputs.json"))?);
        report["elf"] = json!(elf.canonicalize()?);
        report["elf_sha256"] = json!(digest(&code));
        report["objdump_version"] = json!(
            self.output(&["llvm-objdump", "--version"])?
                .lines()
                .next()
                .unwrap_or("")
        );
        if let Some(path) = report_path {
            write_json(path, &report)?;
        }
        println!(
            "reader-stack {} accounted={} available={} reserve={} selected_frames={}/{} frontier_sites={}",
            report["verdict"].as_str().unwrap(),
            report["accounted_bytes_with_reserve"],
            sizes[0],
            stack::RESERVE,
            report["frames"].as_object().unwrap().len(),
            report["selected_symbol_count"],
            report["frontier"].as_array().unwrap().len()
        );
        for (name, error) in report["unsupported_frames"].as_object().unwrap() {
            println!("UNPROVEN: {name}: {error}");
        }
        for limitation in stack::LIMITATIONS {
            println!("LIMIT: {limitation}");
        }
        ensure!(
            report["verdict"] == "PASS_LIMITED",
            "reader stack proof blocked; no passing budget or whole-program bound"
        );
        ensure!(
            !complete,
            "whole-program proof unavailable; selected-frame evidence is incomplete"
        );
        Ok(report)
    }
    pub fn produce(&self, directory: &Path) -> Result<()> {
        self.check_output_directory(directory)?;
        let metadata = self.metadata()?;
        let inputs = self.stable_inputs(&metadata)?;
        let parsed: Metadata = serde_json::from_value(metadata)?;
        let packages: Vec<_> = parsed
            .packages
            .iter()
            .filter(|p| p.name == "miniz_oxide")
            .collect();
        ensure!(
            packages.len() == 1,
            "expected one resolved miniz_oxide dependency"
        );
        let contract = verify_contract(packages[0], &contract_sources())?;
        fs::create_dir_all(directory.parent().context("missing evidence parent")?)?;
        fs::create_dir(directory)?;
        let mut bundle = Bundle {
            schema: 2,
            state: "building".into(),
            inputs: inputs.clone(),
            symbols: Map::new(),
            generated_inputs: Map::new(),
            diagnostics: diagnostics(),
            command: build_command(),
            builds: vec![],
            artifacts: Map::new(),
            decompressor_contract: Some(contract),
        };
        let manifest = directory.join("inputs.json");
        println!(
            "reader-memory source_digest={}",
            inputs["source"]["source_digest"]
        );
        for phase in ["frames", "machine"] {
            let arguments = phase_command(phase, &bundle.symbols);
            let clean = clean_command();
            let run = |args: &[String], file: PathBuf| -> Result<std::process::ExitStatus> {
                let log = fs::File::create(file)?;
                Ok(self
                    .command(args)
                    .stdout(Stdio::from(log.try_clone()?))
                    .stderr(Stdio::from(log))
                    .status()?)
            };
            ensure!(
                run(&clean, directory.join(format!("{phase}-clean.log")))?.success(),
                "reader package clean failed"
            );
            write_json(&manifest, &bundle)?;
            let status = run(&arguments, directory.join(format!("{phase}.log")))?;
            bundle.builds.push(Build {
                phase: phase.into(),
                command: arguments,
                clean,
                exit: status.code().unwrap_or(-1),
            });
            write_json(&manifest, &bundle)?;
            ensure!(
                status.success(),
                "reader {phase} link failed; see {}",
                directory.display()
            );
            ensure!(
                self.stable_inputs(&self.metadata()?)? == inputs,
                "reader source/compiler/dependencies changed during the links"
            );
            let elf = directory.join(if phase == "frames" {
                "frames.elf"
            } else {
                "reader.elf"
            });
            fs::copy(
                parsed
                    .target_directory
                    .join(TARGET)
                    .join("release/brewthink"),
                &elf,
            )?;
            let mut permissions = fs::metadata(&elf)?.permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&elf, permissions)?;
            let generated = generated_inputs(&parsed.target_directory)?;
            if phase == "frames" {
                let code = fs::read(&elf)?;
                bundle.symbols = self.inventory(
                    &elf,
                    &stack::functions_from(
                        &self.disassemble(&elf)?,
                        &stack::function_extents(&code)?,
                    )?,
                )?;
                bundle.generated_inputs = generated;
            } else {
                ensure!(
                    generated == bundle.generated_inputs
                        && fs::read(&elf)? == fs::read(directory.join("frames.elf"))?,
                    "diagnostic links changed generated inputs or intended firmware bytes"
                );
            }
        }
        bundle.artifacts = ARTIFACTS
            .iter()
            .map(|name| Ok(((*name).into(), hash(directory.join(name))?)))
            .collect::<Result<_>>()?;
        bundle.state = "linked".into();
        write_json(&manifest, &bundle)?;
        let elf = directory.join("reader.elf");
        self.validate_bundle(directory, &elf)?;
        self.analyze(&elf, directory, Some(&directory.join("stack.json")), false)?;
        self.validate_bundle(directory, &elf)?;
        write_json(
            directory.join("verified.json"),
            &json!({"inputs_sha256":hash(&manifest)?,"stack_sha256":hash(directory.join("stack.json"))?,"elf_sha256":bundle.artifacts["reader.elf"]}),
        )?;
        self.verify_proof(directory, &elf)?;
        println!("reader-memory evidence={}", directory.display());
        Ok(())
    }
}
pub fn symbol_inventory(
    elf: &[u8],
    functions: &stack::Functions,
    nm: &str,
) -> Result<Map<String, Symbol>> {
    stack::readonly_sections(elf)?;
    let mut code = vec![];
    for s in stack::sections(elf)? {
        if s.kind == 1 && s.flags & 6 == 6 {
            ensure!(
                s.offset + s.size <= elf.len(),
                "truncated executable section"
            );
            code.push((s.address, &elf[s.offset..s.offset + s.size]));
        }
    }
    let mut symbols: Map<u32, Vec<(u32, String)>> = Map::new();
    for line in nm.lines() {
        if let Some(m) = re(r"^([0-9a-f]+) ([0-9a-f]+) [tT] (\S+)$").captures(line) {
            symbols
                .entry(u32::from_str_radix(&m[1], 16)?)
                .or_default()
                .push((u32::from_str_radix(&m[2], 16)?, m[3].into()));
        }
    }
    let poll = stack::reader_symbols(functions)?;
    let mut result = Map::new();
    for (name, body) in functions {
        if !stack::SCOPES
            .iter()
            .any(|s| name.trim_start_matches('<').starts_with(s))
            && name != &poll
        {
            continue;
        }
        let parsed = stack::instructions(body)?;
        let address = parsed[0].0;
        let candidates = symbols.get(&address).context("missing raw symbol extent")?;
        ensure!(
            candidates.len() == 1 && candidates[0].0 > 0,
            "missing/ambiguous raw symbol extent: {name}"
        );
        let (size, raw) = &candidates[0];
        let sections: Vec<_> = code
            .iter()
            .filter(|(base, data)| {
                *base <= address
                    && u64::from(address) + u64::from(*size) <= u64::from(*base) + data.len() as u64
            })
            .collect();
        ensure!(
            sections.len() == 1,
            "function lacks a unique complete executable extent"
        );
        let (base, data) = sections[0];
        let function = &data[(address - base) as usize..(address - base + size) as usize];
        let mut offset = 0usize;
        let mut addresses = vec![];
        while offset < *size as usize {
            ensure!(
                offset + 2 <= *size as usize,
                "truncated emitted instruction"
            );
            let halfword = u16::from_le_bytes(function[offset..offset + 2].try_into()?);
            ensure!(
                halfword & 31 != 31,
                "unsupported greater-than-32-bit instruction"
            );
            addresses.push(address + offset as u32);
            offset += if halfword & 3 == 3 { 4 } else { 2 };
        }
        ensure!(
            offset == *size as usize && addresses == parsed.iter().map(|p| p.0).collect::<Vec<_>>(),
            "disassembly does not decode every byte of selected extent: {name}"
        );
        ensure!(
            result
                .insert(
                    raw.clone(),
                    Symbol {
                        name: name.clone(),
                        address,
                        size: *size,
                        code_sha256: digest(function)
                    }
                )
                .is_none(),
            "duplicate compiler record"
        );
    }
    Ok(result)
}

pub fn contract_sources() -> Hashes {
    [
        (
            "src/lib.rs",
            "87da790f8aeba6da7d9f9ef3e2b48a0d6ad785a0199666754f03b4d1aa3973dc",
        ),
        (
            "src/inflate/core.rs",
            "41a52ea2145885701df41e77b4427aecfa0669971bbc7abb7c90bfe2cfe9fd74",
        ),
        (
            "src/inflate/stream.rs",
            "e6dd9e06e927dbe5a59299db1bc96bf1c55cad8ef995fccec314ecf056622884",
        ),
        (
            "src/inflate/mod.rs",
            "24a041eeb9404079395fd8ac5a51b8e2494fdf46a511681bb727e0b2b7017b57",
        ),
    ]
    .into_iter()
    .map(|(name, hash)| (name.into(), hash.into()))
    .collect()
}
