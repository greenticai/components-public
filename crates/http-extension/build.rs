use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = PathBuf::from(&manifest_dir).join("runtime-version.txt");
    let v = fs::read_to_string(&path)
        .expect("runtime-version.txt missing")
        .trim()
        .to_string();
    println!("cargo:rustc-env=GREENTIC_HTTP_RUNTIME_VERSION={v}");
    println!("cargo:rerun-if-changed=runtime-version.txt");
}
