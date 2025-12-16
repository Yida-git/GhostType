use std::process::Command;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=libopus");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir);
    let static_lib_path = out_dir.join("lib/libopus.a");

    if std::fs::metadata(static_lib_path).is_err() {
        let prefix = safe_install_prefix(out_dir).unwrap_or_else(|| out_dir.to_path_buf());
        build(&prefix);
    }

    println!("cargo:root={}", out_dir.display());
    inform_cargo(out_dir);
}

fn safe_install_prefix(out_dir: &Path) -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let out_str = out_dir.to_string_lossy();
        let mut hasher = DefaultHasher::new();
        out_str.hash(&mut hasher);
        let hash = hasher.finish();

        let link_path = std::env::temp_dir().join(format!("ghosttype_opus_out_{hash:x}"));
        match std::fs::read_link(&link_path) {
            Ok(existing) if existing == out_dir => return Some(link_path),
            Ok(_) => {
                let _ = std::fs::remove_file(&link_path);
            }
            Err(_) => {}
        }

        if std::os::unix::fs::symlink(out_dir, &link_path).is_ok() {
            return Some(link_path);
        }
    }

    None
}

#[cfg(windows)]
fn build(out_dir: &Path) {
    std::env::set_current_dir("libopus").unwrap_or_else(|e| panic!("{}", e));

    success_or_panic(Command::new("sh")
        .args(&["./configure",
                "--disable-shared", "--enable-static",
                "--disable-doc",
                "--disable-extra-programs",
                "--with-pic",
                "--prefix", &out_dir.to_str().unwrap().replace("\\", "/")]));
    success_or_panic(&mut Command::new("make"));
    success_or_panic(&mut Command::new("make").arg("install"));

    std::env::set_current_dir("..").unwrap_or_else(|e| panic!("{}", e));
}

#[cfg(unix)]
fn build(out_dir: &Path) {
    std::env::set_current_dir("libopus").unwrap_or_else(|e| panic!("{}", e));

    success_or_panic(Command::new("./configure")
        .args(&["--disable-shared", "--enable-static",
                "--disable-doc",
                "--disable-extra-programs",
                "--with-pic",
                "--prefix", out_dir.to_str().unwrap()]));
    success_or_panic(&mut Command::new("make"));
    success_or_panic(&mut Command::new("make").arg("install"));

    std::env::set_current_dir("..").unwrap_or_else(|e| panic!("{}", e));
}

fn inform_cargo(out_dir: &Path) {
    println!("cargo:rustc-link-search=native={}/lib", out_dir.display());
    println!("cargo:rustc-link-lib=static=opus");
    #[cfg(any(unix, target_env = "gnu"))]
    println!("cargo:rustc-link-lib=m");
}

fn success_or_panic(cmd: &mut Command) {
    match cmd.output() {
        Ok(output) => if !output.status.success() {
            panic!("command exited with failure\n=== Stdout ===\n{}\n=== Stderr ===\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr))
        },
        Err(e)     => panic!("{}", e),
    }
}
