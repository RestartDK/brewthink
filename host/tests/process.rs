mod support;

use std::{
    panic::catch_unwind,
    process::Command,
    time::{Duration, Instant},
};
use support::{root, run, success};

#[test]
fn timeout_covers_descendants_holding_output_pipes() {
    let started = Instant::now();
    let failure = catch_unwind(|| {
        success(run(
            Command::new("/bin/sh")
                .current_dir(root())
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .args(["-c", "sleep 3 & exit 0"]),
            Duration::from_millis(100),
        ))
    })
    .expect_err("a descendant kept the output pipes open past the deadline");
    let message = failure.downcast_ref::<String>().unwrap();
    assert!(message.contains("timed out"), "{message}");
    assert!(started.elapsed() < Duration::from_secs(2));
}
