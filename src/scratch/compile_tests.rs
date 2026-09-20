extern crate std;

use std::{env, format, fs, path::PathBuf, process::Command, string::String};

#[test]
fn scratch_ownership_is_enforced_at_compile_time() {
    let compiler = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let version = Command::new(&compiler).arg("-vV").output().unwrap();
    assert!(version.status.success());
    let version = String::from_utf8(version.stdout).unwrap();
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("scratch-compile-tests")
        .join(format!("{}", std::process::id()));
    fs::create_dir_all(&output).unwrap();

    for (name, body, error) in [
        (
            "valid_reuse",
            "fn main() { let mut s = scratch::Scratch::<16>::new(); let p = unsafe { s.initialize(zero::<[u8; 16]>) }; p[0] = 1; s.bytes()[0] = 2; }",
            None,
        ),
        (
            "oversized",
            "fn main() { let mut s = scratch::Scratch::<8>::new(); unsafe { s.initialize(zero::<[u8; 16]>); } }",
            Some("E0080"),
        ),
        (
            "overaligned",
            "#[repr(align(16))] struct Aligned(u8); fn main() { let mut s = scratch::Scratch::<64>::new(); unsafe { s.initialize(zero::<Aligned>); } }",
            Some("E0080"),
        ),
        (
            "needs_drop",
            "struct Owned; impl Drop for Owned { fn drop(&mut self) {} } fn main() { let mut s = scratch::Scratch::<16>::new(); unsafe { s.initialize(zero::<Owned>); } }",
            Some("E0080"),
        ),
        (
            "overlapping_borrow",
            "fn main() { let mut s = scratch::Scratch::<16>::new(); let p = unsafe { s.initialize(zero::<[u8; 16]>) }; s.bytes()[0] = 2; p[0] = 1; }",
            Some("E0499"),
        ),
    ] {
        let source = output.join(format!("{name}.rs"));
        fs::write(&source, format!(
            "#[path = {:?}] mod scratch;\nunsafe fn zero<T>(pointer: *mut T) {{ unsafe {{ pointer.write_bytes(0, 1) }} }}\n{body}",
            root.join("src/scratch.rs"),
        )).unwrap();
        let result = Command::new(&compiler)
            .args([
                "--edition=2024",
                "--target",
                host,
                "--emit=obj",
                "--out-dir",
            ])
            .arg(&output)
            .arg(&source)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&result.stderr);
        match error {
            None => assert!(result.status.success(), "{name}: {stderr}"),
            Some(code) => {
                assert!(!result.status.success(), "{name} unexpectedly compiled");
                assert!(
                    stderr.contains(code),
                    "{name}: expected {code}, got {stderr}"
                );
            }
        }
    }
}
