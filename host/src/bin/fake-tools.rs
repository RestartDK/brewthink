use anyhow::{Context, Result, bail, ensure};
use brewthink_host::memory::digest;
use serde_json::json;
use std::{
    env, fs,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
const HEADER: &str = "ESP32-C3 Image Header\nFlash size: 16MB\nFlash freq: 80m\nFlash mode: DIO\nChip ID: 5 (ESP32-C3)\nChecksum: aa (valid)\nValidation hash: aa (valid)\nApplication Information\nProject name: brewthink";
fn flag(name: &str) -> bool {
    env::var_os(name).is_some()
}
fn log(args: impl serde::Serialize) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(env::var("FAKE_LOG")?)?;
    writeln!(file, "{}", serde_json::to_string(&args)?)?;
    Ok(())
}
fn number(text: &str) -> Result<u64> {
    Ok(if let Some(s) = text.strip_prefix("0x") {
        u64::from_str_radix(s, 16)?
    } else {
        text.parse()?
    })
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let name = Path::new(&args[0]).file_name().unwrap().to_str().unwrap();
    let args = &args[1..];
    let sandbox = fs::canonicalize(
        env::var("FAKE_SANDBOX").context("fake-tools requires an isolated test directory")?,
    )?;
    ensure!(
        Path::new(&env::var("FAKE_LOG")?).starts_with(&sandbox),
        "log must remain in the test directory"
    );
    log(std::iter::once(name)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>())?;
    if name == "host-tool.sh" {
        ensure!(
            args.first().map(String::as_str) == Some("memory"),
            "unexpected host tool"
        );
        let evidence = Path::new(&args[1]);
        if args.iter().any(|a| a == "--verify-elf") {
            let elf = Path::new(args.last().unwrap());
            ensure!(
                elf == evidence.join("reader.elf") && fs::read(elf)? == b"proved reader ELF",
                "wrong proved image"
            );
            ensure!(!flag("FAIL_VERIFY"), "proof verification failed");
        } else {
            ensure!(!flag("FAIL_PRODUCER"), "proof production failed");
            fs::create_dir(evidence)?;
            if !flag("MISSING_ELF") {
                fs::write(evidence.join("reader.elf"), b"proved reader ELF")?;
            }
            fs::write(
                evidence.join("stack.json"),
                "{\"verdict\":\"PASS_LIMITED\",\"whole_program_bound\":false}\n",
            )?;
        }
        return Ok(());
    }
    if name == "cargo" {
        if args[0] == "check" {
            ensure!(
                env::var("BREWTHINK_DIAGNOSTIC_STAGE")? == "reader-app"
                    && env::var("BREWTHINK_PREVIOUS_FRAME_STORAGE")? == "host-ram",
                "unexpected compiler probe"
            );
            bail!("reader-app requires controller-ram previous-frame storage")
        }
        ensure!(
            args[0] == "build"
                && env::var("BREWTHINK_DIAGNOSTIC_STAGE").unwrap_or_default() != "reader-app",
            "unexpected build"
        );
        fs::write(env::var("FAKE_TARGET_ELF")?, b"unproved diagnostic ELF")?;
        return Ok(());
    }
    if name == "esptool" {
        ensure!(
            flag("FAKE_FLASH") || args.get(2).map(String::as_str) == Some("image-info"),
            "hardware operations are forbidden in image tests"
        );
        if args.iter().any(|a| a == "flash-id") {
            println!("Manufacturer: 85\nDevice: 2018");
        } else if args.iter().any(|a| a == "image-info") {
            ensure!(!flag("INVALID_IMAGE"), "invalid image");
            println!("{HEADER}");
        } else {
            bail!("unexpected esptool command")
        }
        return Ok(());
    }
    ensure!(name == "espflash", "unexpected tool");
    if args[0] == "save-image" {
        ensure!(!flag("FAIL_SAVE"), "image generation failed");
        fs::copy(&args[args.len() - 2], &args[args.len() - 1])?;
        return Ok(());
    }
    let flash = Path::new(
        &env::var("FAKE_FLASH").context("hardware operations are forbidden in image tests")?,
    )
    .to_path_buf();
    ensure!(
        flash.starts_with(&sandbox)
            && fs::metadata(&flash)?.is_file()
            && fs::metadata(&flash)?.len() == 0x1000000,
        "flash must be a 16 MiB test file"
    );
    match args[0].as_str() {
        "board-info" => {
            println!(
                "Chip type: esp32c3\nFlash size: {}\nCrystal frequency: 40 MHz\nSecure Boot: Disabled\nFlash Encryption: Disabled",
                env::var("FAKE_SIZE").unwrap_or_else(|_| "16MB".into())
            );
            for name in ["MUTATE_SOURCE", "MUTATE_ELF"] {
                if let Ok(path) = env::var(name) {
                    ensure!(
                        Path::new(&path).starts_with(&sandbox),
                        "mutation escaped sandbox"
                    );
                    fs::write(path, b"changed")?;
                }
            }
        }
        "write-bin" => {
            ensure!(!flag("FAIL_WRITE"), "write failed");
            let offset = number(&args[args.len() - 2])?;
            let data = fs::read(args.last().unwrap())?;
            ensure!(
                offset + data.len() as u64 <= 0x1000000,
                "write exceeds test flash"
            );
            let mut file = fs::OpenOptions::new().write(true).open(&flash)?;
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(&data)?;
            if args.iter().any(|a| a == "--monitor") {
                log(json!(["espflash", "monitor"]))?;
            }
        }
        "read-flash" => {
            let offset = number(&args[args.len() - 3])?;
            let size = number(&args[args.len() - 2])?;
            ensure!(offset + size <= 0x1000000, "read exceeds test flash");
            let mut file = fs::File::open(&flash)?;
            file.seek(SeekFrom::Start(offset))?;
            let mut data = vec![0; size as usize];
            file.read_exact(&mut data)?;
            if flag("CORRUPT_READBACK") {
                data[0] ^= 1;
            }
            fs::write(args.last().unwrap(), data)?;
        }
        "erase-region" => bail!("test attempted a forbidden erase"),
        "monitor" => {
            let position = args
                .iter()
                .position(|a| a == "--elf")
                .context("missing monitor ELF")?;
            log(json!([
                "monitor-elf",
                digest(fs::read(&args[position + 1])?)
            ]))?;
        }
        "reset" => {}
        _ => bail!("unexpected espflash command"),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
