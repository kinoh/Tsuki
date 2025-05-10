use std::process::Command;

fn main() {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    let cargo_lock_raw = include_str!("Cargo.lock");
    if cargo_lock_raw
        .split("\n")
        .any(|line| line.starts_with("name = \"openssl\""))
    {
        eprintln!("❌ openssl dependency detected");
        panic!()
    }

    println!("cargo::rustc-link-search=/usr/local/lib/");
}
