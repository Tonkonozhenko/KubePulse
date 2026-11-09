use kubepulse_ui::{RunOptions, run_with};
use tracing_subscriber::filter::LevelFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .with_writer(std::io::stderr)
        .init();
    #[cfg(unix)]
    adopt_login_shell_path();
    run_with(RunOptions {
        extensions: kubepulse_builtin::extensions(),
        ..Default::default()
    });
}

/// Apps opened from a desktop launcher inherit only the session `PATH`, so
/// credential helpers installed elsewhere are not found. The user's login shell
/// knows the full `PATH`; its entries come first, followed by any the process
/// already had.
#[cfg(unix)]
fn adopt_login_shell_path() {
    use std::env;
    use std::process::{Command, Stdio};

    const MARKER: &str = "__KUBEPULSE_PATH__";
    let shell = env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let output = Command::new(&shell)
        .args(["-i", "-l", "-c"])
        .arg(format!("echo {MARKER}; /usr/bin/printenv PATH"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let shell_path = match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .rsplit_once(MARKER)
            .map(|(_, path)| path.trim().to_owned()),
        Ok(output) => {
            tracing::warn!(shell = ?shell, status = %output.status, "login shell failed to report PATH");
            return;
        }
        Err(error) => {
            tracing::warn!(shell = ?shell, %error, "login shell could not be started to read PATH");
            return;
        }
    };
    let Some(shell_path) = shell_path.filter(|path| !path.is_empty()) else {
        tracing::warn!(shell = ?shell, "login shell did not report PATH");
        return;
    };
    let current = env::var_os("PATH").unwrap_or_default();
    let mut entries: Vec<_> = env::split_paths(&shell_path).collect();
    for entry in env::split_paths(&current) {
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    let path = env::join_paths(entries).expect("PATH entries come from PATH values");
    // SAFETY: runs before the UI and the async runtime start any other thread.
    unsafe { env::set_var("PATH", path) };
}
