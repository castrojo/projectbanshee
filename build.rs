// Banshee uses calendar versions YY.MM.patch (25.09.1). Cargo's SemVer forbids the month's
// leading zero, so Cargo.toml holds 25.9.1; re-pad it here for the user-facing string, the way
// Helix does (helix-loader/build.rs).
fn main() {
    let version = format!(
        "{}.{:0>2}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH"),
    );
    println!("cargo:rustc-env=BANSHEE_VERSION={version}");
}
