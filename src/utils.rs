

use std::env;

pub fn is_linux() -> bool {
    env::consts::OS == "linux"
}

pub fn is_arch_match(arch: &str) -> bool {
    let current_arch = env::consts::ARCH;
    match arch {
        "x86_64" => current_arch == "x86_64",
        "x86" => current_arch == "x86",
        "aarch64" => current_arch == "aarch64",
        "arm" => current_arch == "arm",
        _ => false
    }
}




