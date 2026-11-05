use kubepulse_ui::{RunOptions, run_with};

fn main() {
    run_with(RunOptions {
        extensions: kubepulse_builtin::extensions(),
        ..Default::default()
    });
}
