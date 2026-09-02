use std::{env, error::Error, fs, path::PathBuf};

mod stage_names {
    include!("src/diagnostics/stage_names.rs");
}

const DISPLAY_FRAME_BYTES: u64 = 48_000;
const X4_DRIVE_PROFILES: &[&str] = &["openx4-fast-du", "stock-parity"];
const PREVIOUS_FRAME_STORAGES: &[&str] = &["host-ram", "controller-ram"];
const DISPLAY_REFRESH_MODES: &[&str] = &["automatic", "full-clean", "quick-clean", "differential"];
const GENERATED_IMAGE_NAME: &str = "brewthink-image.bin";

fn main() {
    linker_be_nice();
    for variable in [
        "BREWTHINK_DIAGNOSTIC_STAGE",
        "BREWTHINK_DISPLAY_ROTATION",
        "BREWTHINK_X4_DRIVE_PROFILE",
        "BREWTHINK_PREVIOUS_FRAME_STORAGE",
        "BREWTHINK_DISPLAY_REFRESH",
        "BREWTHINK_IMAGE_FRAME",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    validate_build_selection();
    configure_previous_frame_storage();

    if env::var("TARGET").as_deref() != Ok("riscv32imc-unknown-none-elf") {
        return;
    }

    prepare_image().unwrap_or_else(|error| panic!("failed to prepare display image: {error}"));
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    println!(
        "cargo:rustc-link-arg=--error-handling-script={}",
        env::current_exe().unwrap().display()
    );
}

fn validate_build_selection() {
    if let Ok(stage) = env::var("BREWTHINK_DIAGNOSTIC_STAGE") {
        assert!(
            stage_names::ALL_STAGES.contains(&stage.as_str()),
            "unknown BREWTHINK_DIAGNOSTIC_STAGE={stage:?}; expected one of: {}",
            stage_names::ALL_STAGES.join(", ")
        );
    }
    if let Ok(rotation) = env::var("BREWTHINK_DISPLAY_ROTATION") {
        assert!(
            stage_names::DISPLAY_ROTATIONS.contains(&rotation.as_str()),
            "unknown BREWTHINK_DISPLAY_ROTATION={rotation:?}; expected one of: {}",
            stage_names::DISPLAY_ROTATIONS.join(", ")
        );
    }
    if let Ok(profile) = env::var("BREWTHINK_X4_DRIVE_PROFILE") {
        assert!(
            X4_DRIVE_PROFILES.contains(&profile.as_str()),
            "unknown BREWTHINK_X4_DRIVE_PROFILE={profile:?}; expected one of: {}",
            X4_DRIVE_PROFILES.join(", ")
        );
    }
    if let Ok(storage) = env::var("BREWTHINK_PREVIOUS_FRAME_STORAGE") {
        assert!(
            PREVIOUS_FRAME_STORAGES.contains(&storage.as_str()),
            "unknown BREWTHINK_PREVIOUS_FRAME_STORAGE={storage:?}; expected one of: {}",
            PREVIOUS_FRAME_STORAGES.join(", ")
        );
    }
    if let Ok(mode) = env::var("BREWTHINK_DISPLAY_REFRESH") {
        assert!(
            DISPLAY_REFRESH_MODES.contains(&mode.as_str()),
            "unknown BREWTHINK_DISPLAY_REFRESH={mode:?}; expected one of: {}",
            DISPLAY_REFRESH_MODES.join(", ")
        );
    }
}

fn configure_previous_frame_storage() {
    let storage = env::var("BREWTHINK_PREVIOUS_FRAME_STORAGE")
        .unwrap_or_else(|_| "controller-ram".to_owned());
    let value = match storage.as_str() {
        "host-ram" => "host_ram",
        "controller-ram" => "controller_ram",
        _ => unreachable!(),
    };
    println!(
        "cargo:rustc-check-cfg=cfg(brewthink_previous_frame_storage, values(\"host_ram\", \"controller_ram\"))"
    );
    println!("cargo:rustc-cfg=brewthink_previous_frame_storage=\"{value}\"");
}

fn prepare_image() -> Result<(), Box<dyn Error>> {
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?)
        .join(GENERATED_IMAGE_NAME);

    if env::var("BREWTHINK_DIAGNOSTIC_STAGE").as_deref() != Ok(stage_names::DISPLAY_IMAGE) {
        fs::write(output, [0xFF; DISPLAY_FRAME_BYTES as usize])?;
        return Ok(());
    }

    let input = PathBuf::from(
        env::var_os("BREWTHINK_IMAGE_FRAME")
            .ok_or("BREWTHINK_IMAGE_FRAME is required for the image stage")?,
    );
    println!("cargo:rerun-if-changed={}", input.display());

    let actual = fs::metadata(&input)?.len();
    if actual != DISPLAY_FRAME_BYTES {
        return Err(
            format!("packed image must be {DISPLAY_FRAME_BYTES} bytes, got {actual}").into(),
        );
    }

    fs::copy(input, output)?;
    Ok(())
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                "free"
                | "malloc"
                | "calloc"
                | "get_free_internal_heap_size"
                | "malloc_internal"
                | "realloc_internal"
                | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }
}
