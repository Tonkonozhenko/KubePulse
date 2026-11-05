//! Measures launch to the first painted frame, with an unreachable cluster in
//! the kubeconfig and a full themes folder. Exits non-zero over the limit.
//!
//! ```bash
//! cargo run -p kubepulse-ui --release --example startup_check
//! ```

use std::{env, fs, process, time::Duration};

use kubepulse_ui::{RunOptions, persistence::ConfigPaths, run_with};

const LIMIT: Duration = Duration::from_secs(1);
const THEME_FILES: usize = 50;

fn main() {
    let directory = env::temp_dir().join(format!("kubepulse-startup-{}", process::id()));
    fs::create_dir_all(&directory).expect("create a temporary directory");
    // 192.0.2.0/24 is reserved for documentation, so nothing answers there.
    let kubeconfig = directory.join("kubeconfig");
    fs::write(
        &kubeconfig,
        "apiVersion: v1\nkind: Config\nclusters:\n- name: gone\n  cluster:\n    server: https://192.0.2.1:6443\ncontexts:\n- name: gone\n  context:\n    cluster: gone\n    user: gone\nusers:\n- name: gone\n  user: {}\n",
    )
    .expect("write the kubeconfig");

    let config = ConfigPaths::in_directory(&directory);
    fs::create_dir_all(&config.themes).expect("create the themes folder");
    for index in 0..THEME_FILES {
        let theme = format!(
            r#"{{ "name": "Set {index}", "themes": [{{ "name": "Theme {index}", "mode": "dark" }}] }}"#
        );
        fs::write(config.themes.join(format!("{index}.json")), theme).expect("write a theme");
    }

    let cleanup = directory.clone();
    run_with(RunOptions {
        config: Some(config),
        kubeconfig: Some(vec![kubeconfig]),
        extensions: Vec::new(),
        exit_after_first_frame: Some(Box::new(move |elapsed| {
            println!("first frame after {elapsed:?}, limit {LIMIT:?}");
            let _ = fs::remove_dir_all(&cleanup);
            if elapsed > LIMIT {
                process::exit(1);
            }
        })),
    });
}
