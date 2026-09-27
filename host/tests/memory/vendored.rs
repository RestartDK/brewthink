use super::*;

struct Vendor {
    _dir: TempDir,
    root: PathBuf,
    package: Package,
    source: SourceInputs,
}

impl Vendor {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let path = root.join("vendor/embedded-sdmmc-0.10.0");
        fs::create_dir_all(path.join("src")).unwrap();
        let mut hashes = BTreeMap::new();
        for (name, bytes) in [("Cargo.toml", "manifest"), ("src/lib.rs", "library")] {
            let file = path.join(name);
            fs::write(&file, bytes).unwrap();
            hashes.insert(
                file.strip_prefix(&root).unwrap().to_str().unwrap().into(),
                Some(digest(bytes.as_bytes())),
            );
        }
        Self {
            _dir: dir,
            root,
            package: Package {
                name: "embedded-sdmmc".into(),
                version: "0.10.0".into(),
                manifest_path: path.join("Cargo.toml"),
                id: "vendored-sd".into(),
                source: None,
            },
            source: SourceInputs {
                head: "head".into(),
                diff_sha256: "diff".into(),
                files_sha256: hashes,
                source_digest: "source".into(),
            },
        }
    }

    fn check(&self) -> anyhow::Result<DependencyInputs> {
        vendored_sources(&self.package, &self.root, &self.source)
    }
}

#[test]
fn the_reviewed_vendor_is_bound_to_the_workspace_snapshot() {
    let vendor = Vendor::new();
    assert!(matches!(
        vendor.check().unwrap(),
        DependencyInputs::Vendored { .. }
    ));
    fs::write(
        vendor
            .package
            .manifest_path
            .parent()
            .unwrap()
            .join("src/lib.rs"),
        "changed",
    )
    .unwrap();
    error(vendor.check(), "differs from the workspace snapshot");
}

#[test]
fn ignored_or_extra_vendor_files_cannot_escape_the_source_inventory() {
    let vendor = Vendor::new();
    fs::write(
        vendor
            .package
            .manifest_path
            .parent()
            .unwrap()
            .join("extra.rs"),
        "extra",
    )
    .unwrap();
    error(vendor.check(), "missing or differs");
}

#[test]
fn arbitrary_path_dependencies_and_versions_still_require_review() {
    let mut vendor = Vendor::new();
    vendor.package.version = "0.10.1".into();
    error(vendor.check(), "unreviewed path dependency");
    vendor.package.version = "0.10.0".into();
    vendor.package.name = "other".into();
    error(vendor.check(), "unreviewed path dependency");
}

#[cfg(unix)]
#[test]
fn symlinked_vendor_files_and_directories_are_rejected() {
    let vendor = Vendor::new();
    let path = vendor.package.manifest_path.parent().unwrap();
    std::os::unix::fs::symlink(vendor.root.join("outside.rs"), path.join("link.rs")).unwrap();
    error(vendor.check(), "symlink or special file");
    fs::remove_file(path.join("link.rs")).unwrap();
    std::os::unix::fs::symlink(&vendor.root, path.join("linked-directory")).unwrap();
    error(vendor.check(), "symlink or special file");
}
