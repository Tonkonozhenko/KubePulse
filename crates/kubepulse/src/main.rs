use kubepulse_ui::{RunOptions, run_with};
use tracing_subscriber::filter::LevelFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .with_writer(std::io::stderr)
        .init();
    run_with(RunOptions {
        extensions: kubepulse_builtin::extensions(),
        ..Default::default()
    });
}
