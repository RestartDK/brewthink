mod support;
use brewthink_host::memory::digest;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};
use support::*;
use tempfile::TempDir;
fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}
fn ota(sequence: u32, state: u32) -> Vec<u8> {
    let mut data = vec![255; 0x2000];
    data[..4].copy_from_slice(&sequence.to_le_bytes());
    data[24..28].copy_from_slice(&state.to_le_bytes());
    let mut crc = crc32fast::Hasher::new_with_initial(u32::MAX);
    crc.update(&data[..4]);
    data[28..32].copy_from_slice(&crc.finalize().to_le_bytes());
    data
}
fn at(path: &Path, offset: u64, data: &[u8]) {
    let mut f = fs::OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(offset)).unwrap();
    f.write_all(data).unwrap();
}
fn bytes(path: &Path, offset: u64, size: usize) -> Vec<u8> {
    let mut f = fs::File::open(path).unwrap();
    f.seek(SeekFrom::Start(offset)).unwrap();
    let mut data = vec![0; size];
    f.read_exact(&mut data).unwrap();
    data
}
enum FixtureMode {
    Flash,
    ImageBuilder,
}

fn copy_scripts(destination: &Path) {
    fs::create_dir_all(destination.join("scripts")).unwrap();
    for entry in fs::read_dir(root().join("scripts")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(
                entry.path(),
                destination.join("scripts").join(entry.file_name()),
            )
            .unwrap();
        }
    }
    fs::create_dir(destination.join("docs")).unwrap();
    fs::copy(
        root().join("docs/x4-stock-partition-table.csv"),
        destination.join("docs/x4-stock-partition-table.csv"),
    )
    .unwrap();
}

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
    env: BTreeMap<String, String>,
    image: PathBuf,
    elf: PathBuf,
    backup: PathBuf,
    flash: PathBuf,
    log: PathBuf,
}
impl Fixture {
    fn new(mode: FixtureMode) -> Self {
        let image_test = matches!(mode, FixtureMode::ImageBuilder);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        copy_scripts(&root);
        let bin = root.join("bin");
        fs::create_dir(&bin).unwrap();
        for tool in if image_test {
            vec!["espflash", "esptool", "cargo"]
        } else {
            vec!["espflash", "esptool"]
        } {
            std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-tools"), bin.join(tool)).unwrap();
        }
        if image_test {
            fs::remove_file(root.join("scripts/host-tool.sh")).unwrap();
            std::os::unix::fs::symlink(
                env!("CARGO_BIN_EXE_fake-tools"),
                root.join("scripts/host-tool.sh"),
            )
            .unwrap();
        }
        let log = root.join("commands.jsonl");
        let flash = root.join("flash.bin");
        fs::write(&flash, vec![255; 0x1000000]).unwrap();
        let image = root.join("image.bin");
        if !image_test {
            fs::write(&image, b"initial").unwrap();
        }
        let elf = root.join("target/riscv32imc-unknown-none-elf/release/brewthink");
        fs::create_dir_all(elf.parent().unwrap()).unwrap();
        fs::write(&elf, b"test-elf").unwrap();
        let backup = root.join("stock.bin");
        let mut stock = vec![255; 0x1000000];
        stock[0xe000..0x10000].copy_from_slice(&ota(1, u32::MAX));
        stock[0x10000..0x10007].copy_from_slice(b"stock!!");
        fs::write(&backup, stock).unwrap();
        let mut env: BTreeMap<String, String> = std::env::vars()
            .filter(|(n, _)| {
                !n.starts_with("BREWTHINK_") && !n.starts_with("FAKE_") && n != "ESPTOOL_PORT"
            })
            .collect();
        env.extend([
            (
                "PATH".into(),
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            ),
            ("ESPFLASH_PORT".into(), "FAKE-PORT".into()),
            ("FAKE_SANDBOX".into(), text(&root).into()),
            ("FAKE_LOG".into(), text(&log).into()),
            ("FAKE_TARGET_ELF".into(), text(&elf).into()),
        ]);
        if !image_test {
            env.insert("FAKE_FLASH".into(), text(&flash).into());
        }
        Self {
            _dir: dir,
            root,
            env,
            image,
            elf,
            backup,
            flash,
            log,
        }
    }
    fn run(&self, name: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
        run(
            Command::new("bash")
                .arg(self.root.join("scripts").join(name))
                .args(args)
                .env_clear()
                .envs(&self.env)
                .envs(env.iter().copied()),
            Duration::from_secs(30),
        )
    }
    fn image(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut all = vec!["--image", text(&self.image), "--yes"];
        all.extend_from_slice(args);
        self.run("flash-app1-and-readback.sh", &all, env)
    }
    fn stock(&self, name: &str, env: &[(&str, &str)]) -> Output {
        self.run(
            name,
            &[
                "--stock-flash-backup",
                text(&self.backup),
                "--backup-sha256",
                &digest(fs::read(&self.backup).unwrap()),
                "--yes",
            ],
            env,
        )
    }
    fn ota(&self, name: &str, sequence: u32, extra: &[&str], env: &[(&str, &str)]) -> Output {
        let path = self.root.join("otadata.bin");
        let data = ota(sequence, u32::MAX);
        fs::write(&path, &data).unwrap();
        let hash = digest(&data);
        let mut args = vec!["--backup", text(&path), "--backup-sha256", &hash, "--yes"];
        args.extend_from_slice(extra);
        self.run(name, &args, env)
    }
    fn commands(&self) -> Vec<Vec<String>> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn no_actions(&self, names: &[&str]) {
        assert!(
            self.commands()
                .iter()
                .all(|c| !names.contains(&c[1].as_str())),
            "{:?}",
            self.commands()
        );
    }
    fn writes(&self) -> Vec<String> {
        self.commands()
            .iter()
            .filter(|c| c[1] == "write-bin")
            .map(|c| c[c.len() - 2].clone())
            .collect()
    }
}
#[test]
fn monitor_follows_readback() {
    let f = Fixture::new(FixtureMode::Flash);
    success(f.image(&["--monitor", "--elf", text(&f.elf)], &[]));
    let actions: Vec<_> = f
        .commands()
        .into_iter()
        .filter(|c| c[0] == "espflash")
        .map(|c| c[1].clone())
        .collect();
    assert!(
        actions.iter().position(|a| a == "read-flash").unwrap()
            < actions.iter().position(|a| a == "monitor").unwrap()
    );
}
#[test]
fn app1_accepts_the_reviewed_image_digest() {
    let f = Fixture::new(FixtureMode::Flash);
    success(f.image(
        &["--image-sha256", &digest(fs::read(&f.image).unwrap())],
        &[],
    ));
    assert!(!f.writes().is_empty());
}
#[test]
fn app1_rejects_a_mismatched_digest_before_hardware_access() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(
        !f.image(&["--image-sha256", &"0".repeat(64)], &[])
            .status
            .success()
    );
    assert!(f.commands().is_empty());
}
#[test]
fn monitor_requires_an_explicit_matching_elf() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(!f.image(&["--monitor"], &[]).status.success());
    assert!(f.commands().is_empty());
}
#[test]
fn monitor_uses_the_selected_elf_snapshot() {
    let f = Fixture::new(FixtureMode::Flash);
    let selected = f.root.join("reader.elf");
    fs::write(&selected, b"reviewed reader symbols").unwrap();
    let hash = digest(fs::read(&selected).unwrap());
    success(f.image(
        &["--monitor", "--elf", text(&selected)],
        &[("MUTATE_ELF", text(&selected))],
    ));
    assert!(f.commands().contains(&vec!["monitor-elf".into(), hash]));
}
#[test]
fn reviewed_image_is_not_replaced_by_a_concurrent_build() {
    let f = Fixture::new(FixtureMode::Flash);
    success(f.image(&[], &[("MUTATE_SOURCE", text(&f.image))]));
    assert_eq!(bytes(&f.flash, 0x650000, 7), b"initial");
}
#[test]
fn failed_readback_never_monitors_or_resets() {
    let f = Fixture::new(FixtureMode::Flash);
    let out = f.image(
        &["--monitor", "--elf", text(&f.elf)],
        &[("CORRUPT_READBACK", "1")],
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("readback differs")
            && stderr.contains(&digest(fs::read(&f.image).unwrap()))
    );
    assert!(f.commands().iter().any(|c| c[1] == "read-flash"));
    f.no_actions(&["monitor", "reset"]);
}
#[test]
fn failed_write_never_reads_back() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(!f.image(&[], &[("FAIL_WRITE", "1")]).status.success());
    f.no_actions(&["read-flash", "monitor", "reset"]);
}
#[test]
fn erase_is_disabled_even_with_yes() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(!f.run("erase-app1.sh", &["--yes"], &[]).status.success());
    assert!(f.commands().is_empty());
}
#[test]
fn wrong_hardware_never_writes() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(!f.image(&[], &[("FAKE_SIZE", "4MB")]).status.success());
    f.no_actions(&["write-bin"]);
}
#[test]
fn missing_port_never_probes_hardware() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(!f.image(&[], &[("ESPFLASH_PORT", "")]).status.success());
    assert!(f.commands().iter().all(|c| c[0] != "espflash"));
}
#[test]
fn wrong_backup_digest_never_writes() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(
        !f.run(
            "restore-stock-app0.sh",
            &[
                "--stock-flash-backup",
                text(&f.backup),
                "--backup-sha256",
                &"0".repeat(64),
                "--yes"
            ],
            &[]
        )
        .status
        .success()
    );
    f.no_actions(&["write-bin"]);
}
#[test]
fn write_boundary_rejects_protected_offset() {
    let f = Fixture::new(FixtureMode::Flash);
    let out = run(
        Command::new("bash")
            .args([
                "-c",
                "source \"$1\"; private_workspace; explicit_port; write_and_verify 0x0 7 \"$2\"",
                "test",
                text(&f.root.join("scripts/common.sh")),
                text(&f.image),
            ])
            .env_clear()
            .envs(&f.env),
        Duration::from_secs(30),
    );
    assert!(!out.status.success());
    assert!(f.commands().is_empty());
}
#[test]
fn restore_requires_reviewed_digest() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(
        !f.run(
            "restore-stock-state.sh",
            &["--stock-flash-backup", text(&f.backup), "--yes"],
            &[]
        )
        .status
        .success()
    );
    f.no_actions(&["write-bin"]);
}
#[test]
fn stock_restore_preserves_app1() {
    let f = Fixture::new(FixtureMode::Flash);
    at(&f.flash, 0x650000, b"keep-app1");
    success(f.stock("restore-stock-state.sh", &[]));
    assert_eq!(f.writes(), ["0x10000", "0xE000"]);
    assert_eq!(bytes(&f.flash, 0x650000, 9), b"keep-app1");
    f.no_actions(&["erase-region"]);
}
#[test]
fn stock_restore_rejects_app1_selection_before_any_write() {
    let f = Fixture::new(FixtureMode::Flash);
    at(&f.backup, 0xe000, &ota(2, u32::MAX));
    assert!(!f.stock("restore-stock-state.sh", &[]).status.success());
    f.no_actions(&["write-bin"]);
}
#[test]
fn stock_restore_readback_failure_never_changes_boot_selection() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(
        !f.stock("restore-stock-state.sh", &[("CORRUPT_READBACK", "1")])
            .status
            .success()
    );
    assert_eq!(f.writes(), ["0x10000"]);
    f.no_actions(&["reset"]);
}
#[test]
fn first_switch_checks_live_app1() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(
        !f.ota("switch-boot-app1.sh", 1, &["--image", text(&f.image)], &[])
            .status
            .success()
    );
    f.no_actions(&["write-bin"]);
}
#[test]
fn first_switch_writes_only_second_ota_sector() {
    let f = Fixture::new(FixtureMode::Flash);
    at(&f.flash, 0x650000, &fs::read(&f.image).unwrap());
    at(&f.flash, 0xe000, &ota(1, u32::MAX));
    success(f.ota("switch-boot-app1.sh", 1, &["--image", text(&f.image)], &[]));
    assert_eq!(f.writes(), ["0xF000"]);
    let data = bytes(&f.flash, 0xe000, 0x2000);
    assert_selection(&data, 2, "app1");
    assert_eq!(data[..0x1000], ota(1, u32::MAX)[..0x1000]);
}
#[test]
fn restore_otadata_rejects_wrong_slot() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(
        !f.ota("restore-otadata.sh", 2, &["--expect-slot", "app0"], &[])
            .status
            .success()
    );
    f.no_actions(&["write-bin"]);
}
#[test]
fn restore_otadata_rejects_invalid_target_image() {
    let f = Fixture::new(FixtureMode::Flash);
    assert!(
        !f.ota(
            "restore-otadata.sh",
            1,
            &["--expect-slot", "app0"],
            &[("INVALID_IMAGE", "1")]
        )
        .status
        .success()
    );
    f.no_actions(&["write-bin"]);
}
#[test]
fn restore_otadata_writes_only_metadata() {
    let f = Fixture::new(FixtureMode::Flash);
    success(f.ota("restore-otadata.sh", 1, &["--expect-slot", "app0"], &[]));
    assert_eq!(f.writes(), ["0xE000"]);
}
#[test]
fn restore_app0_writes_only_app0() {
    let f = Fixture::new(FixtureMode::Flash);
    success(f.stock("restore-stock-app0.sh", &[]));
    assert_eq!(f.writes(), ["0x10000"]);
}
#[test]
fn backup_does_not_overwrite_latest() {
    let f = Fixture::new(FixtureMode::Flash);
    let directory = f.root.join("backup/otadata");
    fs::create_dir_all(&directory).unwrap();
    let latest = directory.join("otadata-latest.bin");
    fs::write(&latest, b"preserve").unwrap();
    at(&f.flash, 0xe000, &ota(1, u32::MAX));
    success(f.run("backup-otadata.sh", &[], &[]));
    success(f.run("backup-otadata.sh", &[], &[]));
    assert_eq!(fs::read(latest).unwrap(), b"preserve");
    assert_eq!(
        fs::read_dir(directory)
            .unwrap()
            .filter(|e| e
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|s| s == "sha256"))
            .count(),
        2
    );
}
fn inspect(data: &[u8]) -> Output {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("otadata.bin");
    fs::write(&path, data).unwrap();
    run(
        Command::new("python3")
            .arg(root().join("scripts/inspect-otadata.py"))
            .arg(path),
        Duration::from_secs(5),
    )
}
fn assert_selection(data: &[u8], sequence: u32, slot: &str) {
    let out = success(inspect(data));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        format!("sequence={sequence} slot={slot}")
    );
}
#[test]
fn stock_and_development_sequences() {
    assert_selection(&ota(1, u32::MAX), 1, "app0");
    assert_selection(&ota(2, u32::MAX), 2, "app1");
}
#[test]
fn both_sectors_use_the_larger_sequence() {
    let mut data = ota(3, u32::MAX);
    data[0x1000..].copy_from_slice(&ota(2, u32::MAX)[..0x1000]);
    assert_selection(&data, 3, "app0");
}
#[test]
fn rejects_unconfirmed_and_invalid_states() {
    for state in [0, 1, 3, 4, 5] {
        assert!(!inspect(&ota(1, state)).status.success());
    }
    assert_selection(&ota(1, 2), 1, "app0");
}
#[test]
fn rejects_corruption_and_empty_selection() {
    for data in [
        vec![],
        vec![255; 0x2000],
        ota(0, u32::MAX),
        ota(u32::MAX, u32::MAX),
    ] {
        assert!(!inspect(&data).status.success());
    }
    let mut data = ota(1, u32::MAX);
    data[28] ^= 1;
    assert!(!inspect(&data).status.success());
}
const REPORT: &str = "{\"verdict\":\"PASS_LIMITED\",\"whole_program_bound\":false}\n";
fn sidecar(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.reader-stack.json", path.display()))
}
#[test]
fn builder_packages_the_proved_elf_and_verifies_before_copying_its_report() {
    let f = Fixture::new(FixtureMode::ImageBuilder);
    success(f.run("build-reader-app1.sh", &[text(&f.image)], &[]));
    assert_eq!(fs::read(&f.image).unwrap(), b"proved reader ELF");
    assert_eq!(fs::read_to_string(sidecar(&f.image)).unwrap(), REPORT);
    let commands = f.commands();
    assert_eq!(
        commands.iter().map(|c| c[0].as_str()).collect::<Vec<_>>(),
        ["host-tool.sh", "espflash", "esptool", "host-tool.sh"]
    );
    let evidence = Path::new(&commands[0][2]);
    assert_eq!(
        Path::new(&commands[1][commands[1].len() - 2]),
        evidence.join("reader.elf")
    );
    let last = commands.last().unwrap();
    assert_eq!(
        &last[last.len() - 2..],
        ["--verify-elf", text(&evidence.join("reader.elf"))]
    );
    assert_ne!(fs::read(&f.elf).unwrap(), b"proved reader ELF");
}
#[test]
fn failed_production_or_missing_proved_elf_never_generates_an_image() {
    for failure in ["FAIL_PRODUCER", "MISSING_ELF"] {
        let f = Fixture::new(FixtureMode::ImageBuilder);
        assert!(
            !f.run("build-reader-app1.sh", &[text(&f.image)], &[(failure, "1")])
                .status
                .success()
        );
        assert!(!f.image.exists());
        assert!(f.commands().iter().all(|c| c[0] != "espflash"));
    }
}
#[test]
fn failed_image_generation_never_verifies_or_publishes_a_report() {
    let f = Fixture::new(FixtureMode::ImageBuilder);
    assert!(
        !f.run(
            "build-reader-app1.sh",
            &[text(&f.image)],
            &[("FAIL_SAVE", "1")]
        )
        .status
        .success()
    );
    assert!(
        f.commands()
            .iter()
            .all(|c| !c.iter().any(|a| a == "--verify-elf"))
    );
    assert!(!sidecar(&f.image).exists());
}
#[test]
fn failed_post_image_verification_never_publishes_a_report() {
    let f = Fixture::new(FixtureMode::ImageBuilder);
    assert!(
        !f.run(
            "build-reader-app1.sh",
            &[text(&f.image)],
            &[("FAIL_VERIFY", "1")]
        )
        .status
        .success()
    );
    assert!(f.image.exists());
    assert!(!sidecar(&f.image).exists());
}
#[test]
fn generic_reader_entry_rejects_changed_features_before_any_tool_runs() {
    let f = Fixture::new(FixtureMode::ImageBuilder);
    assert!(
        !f.run(
            "build-app1-image.sh",
            &[text(&f.image)],
            &[
                ("BREWTHINK_DIAGNOSTIC_STAGE", "reader-app"),
                ("BREWTHINK_CARGO_FEATURES", "device-reader,epub")
            ]
        )
        .status
        .success()
    );
    assert!(f.commands().is_empty());
}
#[test]
fn ci_retains_the_reader_report_after_later_diagnostic_builds() {
    let f = Fixture::new(FixtureMode::ImageBuilder);
    success(f.run("check-firmware.sh", &[], &[]));
    assert_eq!(
        fs::read_to_string(f.root.join("artifacts/ci/reader-stack.json")).unwrap(),
        REPORT
    );
    assert_eq!(
        fs::read(f.root.join("artifacts/ci/reader-app-controller-ram.bin")).unwrap(),
        b"proved reader ELF"
    );
    assert_ne!(fs::read(&f.elf).unwrap(), b"proved reader ELF");
    assert_eq!(
        f.commands()
            .iter()
            .filter(|c| c[0] == "host-tool.sh")
            .count(),
        2
    );
}
#[test]
fn ci_stops_without_a_reader_report_if_proof_verification_fails() {
    let f = Fixture::new(FixtureMode::ImageBuilder);
    assert!(
        !f.run("check-firmware.sh", &[], &[("FAIL_VERIFY", "1")])
            .status
            .success()
    );
    assert!(!f.root.join("artifacts/ci/reader-stack.json").exists());
}
