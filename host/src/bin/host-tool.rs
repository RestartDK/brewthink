use anyhow::{Context, Result, bail, ensure};
use brewthink_host::memory::{ReaderBuild, build_environment};
use std::{
    env,
    path::{Path, PathBuf},
};
fn absolute(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    Ok(if path.is_absolute() {
        path.into()
    } else {
        env::current_dir()?.join(path)
    })
}
fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    ensure!(
        args.next().as_deref() == Some("--root"),
        "usage: host-tool --root REPO memory DIRECTORY [--verify-elf ELF] | stack ELF --evidence DIRECTORY [--report FILE] [--require-complete]"
    );
    let root = PathBuf::from(args.next().context("missing repository path")?).canonicalize()?;
    let command = args.next().context("missing command")?;
    let path = absolute(&args.next().context("missing input path")?)?;
    let mut evidence = None;
    let mut report = None;
    let mut verify = None;
    let mut complete = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verify-elf" => {
                ensure!(
                    command == "memory" && verify.is_none(),
                    "unexpected --verify-elf"
                );
                verify = Some(absolute(&args.next().context("missing ELF")?)?);
            }
            "--evidence" => {
                ensure!(
                    command == "stack" && evidence.is_none(),
                    "unexpected --evidence"
                );
                evidence = Some(absolute(
                    &args.next().context("missing evidence directory")?,
                )?);
            }
            "--report" => {
                ensure!(
                    command == "stack" && report.is_none(),
                    "unexpected --report"
                );
                report = Some(absolute(&args.next().context("missing report path")?)?);
            }
            "--require-complete" => {
                ensure!(
                    command == "stack" && !complete,
                    "unexpected --require-complete"
                );
                complete = true;
            }
            _ => bail!("unknown argument: {arg}"),
        }
    }
    let environment = build_environment(&root, env::vars().collect())?;
    let build = ReaderBuild { root, environment };
    match command.as_str() {
        "memory" => {
            if let Some(elf) = verify {
                build.verify_proof(&path, &elf)?;
                println!(
                    "reader-memory verified exact completed PASS_LIMITED artifact; whole_program_bound=false"
                );
            } else {
                build.produce(&path)?;
            }
        }
        "stack" => {
            build.analyze(
                &path,
                &evidence.context("stack requires --evidence")?,
                report.as_deref(),
                complete,
            )?;
        }
        _ => bail!("unknown command: {command}"),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
