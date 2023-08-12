#[cfg(windows)]
fn main() {
    use windres::Build;
    Build::new().compile("tray-icon.rc").unwrap();
}
