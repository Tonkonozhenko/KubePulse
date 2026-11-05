//! A credential helper for tests. A real program rather than a shell script, so
//! the tests run on every platform.
//!
//! Arguments: `<token> <delay-ms> <exit-code> [marker-file]`. Failures print the
//! token, so tests can prove helper output stays out of errors and logs.

use std::{env, fs, process, thread, time::Duration};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let token = &args[0];
    let delay_ms: u64 = args[1].parse().expect("delay in milliseconds");
    let exit_code: i32 = args[2].parse().expect("exit code");

    if let Some(marker) = args.get(3) {
        fs::write(marker, "ran").expect("write marker");
    }
    thread::sleep(Duration::from_millis(delay_ms));
    if exit_code != 0 {
        println!("failed with {token}");
        eprintln!("failed with {token}");
        process::exit(exit_code);
    }
    println!(
        "{{\"apiVersion\":\"client.authentication.k8s.io/v1\",\"kind\":\"ExecCredential\",\"status\":{{\"token\":\"{token}\"}}}}"
    );
}
