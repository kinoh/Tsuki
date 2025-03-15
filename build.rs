fn main() {
    println!("cargo::rustc-link-search=/usr/local/lib/");
    println!("cargo:rerun-if-changed=src/prompt/initial.txt");
}
