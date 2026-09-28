#[path = "memory/vendored.rs"]
mod vendored;

use brewthink_host::memory::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .into()
}
fn error<T: std::fmt::Debug>(result: anyhow::Result<T>, needle: &str) {
    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains(needle), "{error:#}");
}
fn environment(root: &Path) -> Environment {
    BTreeMap::from([(
        "CARGO_HOME".into(),
        root.join("home").to_string_lossy().into(),
    )])
}
struct Evidence {
    dir: TempDir,
    bundle: Bundle,
}
impl Evidence {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        for name in ARTIFACTS {
            fs::write(
                path.join(name),
                if name.ends_with(".elf") {
                    b"same-elf".as_slice()
                } else {
                    b"compiler-records"
                },
            )
            .unwrap();
        }
        let symbols = BTreeMap::from([(
            "reader".into(),
            Symbol {
                name: "reader".into(),
                address: 0,
                size: 1,
                code_sha256: "symbol-hash".into(),
            },
        )]);
        let bundle = Bundle {
            schema: 2,
            state: BundleState::Linked,
            inputs: Inputs {
                source: SourceInputs {
                    head: "current".into(),
                    diff_sha256: "diff".into(),
                    files_sha256: BTreeMap::new(),
                    source_digest: "source".into(),
                },
                compiler: CompilerInputs {
                    rustc: "rustc".into(),
                    cargo: "cargo".into(),
                    sysroot: path.join("sysroot"),
                },
                cwd: path.into(),
                environment: BTreeMap::new(),
                cargo_configuration: BTreeMap::new(),
                metadata_sha256: "metadata".into(),
                dependencies: BTreeMap::new(),
                target_directory: path.join("target"),
            },
            symbols: symbols.clone(),
            generated_inputs: BTreeMap::from([(
                "release/build/test/out/link.x".into(),
                "generated-hash".into(),
            )]),
            diagnostics: diagnostics(),
            command: build_command(),
            builds: BuildPhase::ALL
                .into_iter()
                .map(|phase| Build {
                    phase,
                    command: phase.command(&symbols),
                    clean: clean_command(),
                    exit: 0,
                })
                .collect(),
            artifacts: ARTIFACTS
                .iter()
                .map(|n| ((*n).into(), digest(fs::read(path.join(n)).unwrap())))
                .collect(),
            decompressor_contract: None,
        };
        let fixture = Self { dir, bundle };
        fixture.save(&json!(fixture.bundle));
        fixture
    }
    fn path(&self) -> &Path {
        self.dir.path()
    }
    fn save(&self, value: &Value) {
        write_json(self.path().join("inputs.json"), value).unwrap();
    }
    fn validate(&self) -> anyhow::Result<Bundle> {
        let bundle = validate_artifacts(self.path(), &self.path().join("reader.elf"))?;
        validate_inputs(&bundle, &self.bundle.inputs, &self.bundle.generated_inputs)?;
        Ok(bundle)
    }
    fn complete(&self, verdict: &str, inputs: Option<&str>) {
        let manifest = digest(fs::read(self.path().join("inputs.json")).unwrap());
        let report = json!({"verdict":verdict,"whole_program_bound":false,"elf_sha256":self.bundle.artifacts["reader.elf"],"evidence_inputs_sha256":inputs.unwrap_or(&manifest)});
        write_json(self.path().join("stack.json"), &report).unwrap();
        write_json(self.path().join("verified.json"),&json!({"inputs_sha256":manifest,"elf_sha256":self.bundle.artifacts["reader.elf"],"stack_sha256":digest(fs::read(self.path().join("stack.json")).unwrap())})).unwrap();
    }
}
#[test]
fn complete_matching_bundle_is_accepted() {
    let e = Evidence::new();
    assert_eq!(e.validate().unwrap().symbols, e.bundle.symbols);
}
#[test]
fn partial_duplicate_or_malformed_metadata_is_rejected() {
    let e = Evidence::new();
    for (schema, state) in [
        (json!(1), "linked"),
        (json!(2.0), "linked"),
        (json!(true), "linked"),
        (json!(2), "building"),
        (json!(2), "unknown"),
    ] {
        let mut b = json!(e.bundle);
        b["schema"] = schema;
        b["state"] = json!(state);
        e.save(&b);
        assert!(e.validate().is_err());
    }
    fs::write(e.path().join("inputs.json"), "{\"schema\":2,\"schema\":2}").unwrap();
    error(e.validate(), "duplicate");
}
#[test]
fn evidence_rejects_unknown_phases_and_input_fields() {
    let e = Evidence::new();
    for mutation in ["phase", "diagnostics", "inputs", "compiler", "target"] {
        let mut bundle = json!(e.bundle);
        match mutation {
            "phase" => bundle["builds"][0]["phase"] = json!("unknown"),
            "diagnostics" => bundle["diagnostics"]["unknown"] = json!([]),
            "inputs" => bundle["inputs"]["unknown"] = json!(true),
            "compiler" => bundle["inputs"]["compiler"]["unknown"] = json!(true),
            "target" => bundle["inputs"]["target_directory"] = json!([]),
            _ => unreachable!(),
        }
        e.save(&bundle);
        assert!(e.validate().is_err(), "{mutation}");
    }
}

#[test]
fn each_required_artifact_is_bound() {
    let e = Evidence::new();
    for name in ARTIFACTS {
        let mut b = json!(e.bundle);
        b["artifacts"].as_object_mut().unwrap().remove(*name);
        e.save(&b);
        assert!(e.validate().is_err());
    }
    e.save(&json!(e.bundle));
    for name in ARTIFACTS {
        let path = e.path().join(name);
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"stale-or-corrupt").unwrap();
        error(e.validate(), "changed compiler evidence artifact");
        fs::write(path, original).unwrap();
    }
}
#[test]
fn symlinked_artifacts_are_rejected() {
    let e = Evidence::new();
    let original = e.path().join("original");
    fs::rename(e.path().join("frames.log"), &original).unwrap();
    std::os::unix::fs::symlink(original, e.path().join("frames.log")).unwrap();
    error(e.validate(), "changed compiler evidence artifact");
}
#[test]
fn cross_elf_bundle_and_wrong_requested_image_are_rejected() {
    let e = Evidence::new();
    let changed = e.path().join("frames.elf");
    fs::write(&changed, b"other-link").unwrap();
    let mut b = json!(e.bundle);
    b["artifacts"]["frames.elf"] = json!(digest(b"other-link"));
    e.save(&b);
    error(e.validate(), "exact intended ELF");
    fs::write(changed, b"same-elf").unwrap();
    e.save(&json!(e.bundle));
    let wrong = e.path().join("wrong.elf");
    fs::write(&wrong, b"different-elf").unwrap();
    error(validate_artifacts(e.path(), &wrong), "exact intended ELF");
}
#[test]
fn different_source_or_generated_inputs_are_rejected() {
    let e = Evidence::new();
    let mut current = e.bundle.inputs.clone();
    current.source.head = "changed".into();
    error(
        validate_inputs(&e.bundle, &current, &e.bundle.generated_inputs),
        "inputs changed",
    );
    error(
        validate_inputs(&e.bundle, &e.bundle.inputs, &BTreeMap::new()),
        "generated build inputs changed",
    );
}
#[test]
fn packaging_requires_completed_exact_passing_proof() {
    let e = Evidence::new();
    assert!(verify_completion(e.path(), &e.bundle).is_err());
    e.complete("PASS_LIMITED", None);
    verify_completion(e.path(), &e.bundle).unwrap();
    fs::write(e.path().join("stack.json"), "changed report").unwrap();
    assert!(verify_completion(e.path(), &e.bundle).is_err());
    for (verdict, inputs) in [
        ("BLOCKED_BUDGET", None),
        ("PASS_LIMITED", Some("wrong-inputs")),
    ] {
        e.complete(verdict, inputs);
        error(verify_completion(e.path(), &e.bundle), "does not pass");
    }
}
#[test]
fn missing_clean_failed_build_or_codegen_change_is_rejected() {
    let e = Evidence::new();
    for mutation in ["clean", "exit", "boolean-exit", "codegen", "missing-phase"] {
        let mut b = json!(e.bundle);
        match mutation {
            "missing-phase" => {
                b["builds"].as_array_mut().unwrap().pop();
            }
            "codegen" => b["builds"][1]["command"]
                .as_array_mut()
                .unwrap()
                .extend([json!("-C"), json!("opt-level=0")]),
            "clean" => b["builds"][0]["clean"] = json!([]),
            "exit" => b["builds"][0]["exit"] = json!(1),
            _ => b["builds"][0]["exit"] = json!(false),
        }
        e.save(&b);
        assert!(e.validate().is_err(), "{mutation}");
    }
}

struct Registry {
    _dir: TempDir,
    root: PathBuf,
    archive: PathBuf,
    package: Package,
    checksum: String,
}
impl Registry {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("registry/src/index/sample-0.1.0");
        fs::create_dir_all(root.join("src")).unwrap();
        let data = b"pub fn sample() {}";
        fs::write(root.join("src/lib.rs"), data).unwrap();
        let archive = dir.path().join("registry/cache/index/sample-0.1.0.crate");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        let gzip = flate2::write::GzEncoder::new(
            fs::File::create(&archive).unwrap(),
            flate2::Compression::default(),
        );
        let mut tar = tar::Builder::new(gzip);
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "sample-0.1.0/src/lib.rs", data.as_slice())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        let checksum = digest(fs::read(&archive).unwrap());
        let package = Package {
            name: "sample".into(),
            version: "0.1.0".into(),
            manifest_path: root.join("Cargo.toml"),
            id: "test".into(),
            source: None,
        };
        Self {
            _dir: dir,
            root,
            archive,
            package,
            checksum,
        }
    }
    fn verify(&self) -> anyhow::Result<String> {
        registry_sources(&self.package, &self.checksum)
    }
}
#[test]
fn normal_registry_cache_without_vendor_metadata_is_bound() {
    let r = Registry::new();
    assert!(!r.root.join(".cargo-checksum.json").exists());
    assert_eq!(r.verify().unwrap().len(), 64);
}
#[test]
fn changed_archive_changed_source_and_extra_file_are_rejected() {
    let r = Registry::new();
    let source = r.root.join("src/lib.rs");
    fs::write(&source, "changed").unwrap();
    error(r.verify(), "source differs");
    fs::write(source, "pub fn sample() {}").unwrap();
    let extra = r.root.join("new.rs");
    fs::write(&extra, "extra source").unwrap();
    error(r.verify(), "inventory changed");
    fs::remove_file(extra).unwrap();
    fs::write(&r.archive, "changed archive").unwrap();
    error(r.verify(), "archive checksum");
}
#[test]
fn vendor_checksums_must_match_lock_and_files() {
    let r = Registry::new();
    write_json(
        r.root.join(".cargo-checksum.json"),
        &json!({"package":r.checksum,"files":{"src/lib.rs":digest(b"pub fn sample() {}")}}),
    )
    .unwrap();
    fs::remove_file(&r.archive).unwrap();
    assert_eq!(r.verify().unwrap().len(), 64);
    error(registry_sources(&r.package, &"0".repeat(64)), "Cargo.lock");
}
#[test]
fn version_change_requires_review() {
    let mut r = Registry::new();
    r.package.version = "0.9.2".into();
    error(
        verify_contract(&r.package, &BTreeMap::new()),
        "version changed",
    );
}
#[test]
fn same_version_with_changed_private_source_is_rejected() {
    let mut r = Registry::new();
    r.package.version = "0.9.1".into();
    let path = r.root.join("state.rs");
    fs::write(&path, "reviewed private fields").unwrap();
    let expected = BTreeMap::from([("state.rs".into(), digest(fs::read(&path).unwrap()))]);
    assert_eq!(
        verify_contract(&r.package, &expected).unwrap().files_sha256,
        expected
    );
    fs::write(path, "a new reference field").unwrap();
    error(verify_contract(&r.package, &expected), "source changed");
}
#[test]
fn bootstrap_wrappers_and_codegen_overrides_are_rejected() {
    let d = tempfile::tempdir().unwrap();
    for name in [
        "RUSTC_BOOTSTRAP",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_PROFILE_RELEASE_OPT_LEVEL",
        "CARGO_TARGET_RISCV32IMC_UNKNOWN_NONE_ELF_RUSTFLAGS",
        "CARGO_BUILD_RUSTC",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_INCLUDE",
    ] {
        error(
            build_environment(d.path(), BTreeMap::from([(name.into(), "override".into())])),
            "code-generation override",
        );
    }
}
#[test]
fn cargo_configuration_cannot_select_an_unidentified_compiler() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join(".cargo")).unwrap();
    let path = d.path().join(".cargo/config.toml");
    for text in [
        "[build]\nrustc=\"other\"",
        "[build]\nrustc-wrapper=\"other\"",
        "[build]\nrustc-workspace-wrapper=\"other\"",
        "[env]\nRUSTC_BOOTSTRAP=\"1\"",
        "[env]\nCARGO_BUILD_RUSTC=\"other\"",
        "[env]\nRUSTUP_TOOLCHAIN={value=\"other-installed-toolchain\",force=true}",
        "[env]\nRUSTUP_HOME={value=\"other\",force=true}",
        "[env]\nPATH={value=\"other\",force=true}",
        "[env]\nCARGO_HOME={value=\"other\",force=true}",
        "[env]\nBREWTHINK_DIAGNOSTIC_STAGE={value=\"other\",force=true}",
    ] {
        fs::write(&path, text).unwrap();
        assert!(
            cargo_configuration(d.path(), &environment(d.path())).is_err(),
            "{text}"
        );
    }
}
#[test]
fn cargo_home_must_not_resolve_against_another_working_directory() {
    let d = tempfile::tempdir().unwrap();
    error(
        cargo_configuration(
            d.path(),
            &BTreeMap::from([("CARGO_HOME".into(), "target/cargo-home".into())]),
        ),
        "absolute",
    );
}
#[test]
fn reviewed_logging_configuration_is_hashed() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join(".cargo")).unwrap();
    let path = d.path().join(".cargo/config.toml");
    fs::write(&path, "[env]\nDEFMT_LOG={value=\"info\",force=true}\n").unwrap();
    assert_eq!(
        cargo_configuration(d.path(), &environment(d.path())).unwrap()[path.to_str().unwrap()],
        digest(fs::read(path).unwrap())
    );
}
#[test]
fn cargo_includes_cannot_escape_configuration_binding() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join(".cargo")).unwrap();
    fs::write(
        d.path().join(".cargo/config.toml"),
        "include=[\"other.toml\"]\n",
    )
    .unwrap();
    fs::write(
        d.path().join(".cargo/other.toml"),
        "[build]\nrustc=\"other\"\n",
    )
    .unwrap();
    error(
        cargo_configuration(d.path(), &environment(d.path())),
        "include",
    );
}
#[test]
fn stage_and_storage_are_not_silently_changed() {
    let d = tempfile::tempdir().unwrap();
    for (key, value) in [
        ("BREWTHINK_DIAGNOSTIC_STAGE", "grayscale-bench"),
        ("BREWTHINK_PREVIOUS_FRAME_STORAGE", "host-ram"),
    ] {
        let mut env = environment(d.path());
        env.insert(key.into(), value.into());
        assert!(build_environment(d.path(), env).is_err());
    }
    assert_eq!(
        build_environment(d.path(), environment(d.path())).unwrap()["BREWTHINK_PREVIOUS_FRAME_STORAGE"],
        "controller-ram"
    );
}
#[test]
fn nonignored_output_is_rejected_before_build_inputs_are_read() {
    let d = tempfile::tempdir().unwrap();
    let build = ReaderBuild {
        root: d.path().into(),
        environment: Environment::new(),
    };
    error(
        build.produce(&d.path().join("unignored")),
        "external or gitignored",
    );
}
#[test]
fn reused_evidence_directory_cannot_be_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let marker = d.path().join("keep");
    fs::write(&marker, "existing evidence").unwrap();
    let build = ReaderBuild {
        root: d.path().into(),
        environment: Environment::new(),
    };
    error(build.produce(d.path()), "refusing to overwrite");
    assert_eq!(fs::read_to_string(marker).unwrap(), "existing evidence");
}
#[test]
fn ci_keeps_reader_image_proof_bundles_on_failure() {
    let workflow = fs::read_to_string(root().join(".github/workflows/rust_ci.yml")).unwrap();
    let firmware = workflow
        .split("  firmware:\n")
        .nth(1)
        .unwrap()
        .split("  web-simulator:\n")
        .next()
        .unwrap();
    assert!(firmware.contains("artifacts/ci/*.memory.*/"));
    assert!(firmware.contains("if: always()"));
}
#[test]
fn ci_fetches_locked_dependencies_before_offline_checks() {
    let workflow = fs::read_to_string(root().join(".github/workflows/rust_ci.yml")).unwrap();
    let firmware = workflow
        .split("  firmware:\n")
        .nth(1)
        .unwrap()
        .split("  web-simulator:\n")
        .next()
        .unwrap();
    assert!(
        firmware.find("- run: cargo fetch --locked").unwrap()
            < firmware.find("- run: scripts/check-firmware.sh").unwrap()
    );
}
