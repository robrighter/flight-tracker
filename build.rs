fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/icon/flight-tracker.ico");
    if let Err(error) = res.compile() {
        eprintln!("warning: failed to embed application icon: {error}");
    }
}
