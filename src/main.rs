#![feature(cfg_select)]

mod ui;
use tracing_core::Level;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let filter = args
        .iter()
        .find(|&arg| matches!(arg.as_str(), "--debug" | "-d"))
        .map_or(Level::INFO, |_| Level::DEBUG);
    tracing_subscriber::fmt().with_max_level(filter).init();

    ui::init()
}
