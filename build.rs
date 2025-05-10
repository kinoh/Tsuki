fn main() {
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
