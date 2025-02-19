

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;


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

pub fn find_executable(name: &PathBuf) -> io::Result<(PathBuf, Vec<u8>)> {
    // First try the file directly
    if name.exists() {
        return Ok((name.clone(), fs::read(name)?));
    }

    // If the path is absolute or contains directory components, don't search PATH
    if name.is_absolute() || name.components().count() > 1 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "File not found",
        ));
    }

    // Search in PATH
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            let full_path = dir.join(name);
            if full_path.exists() {
                let buffer = fs::read(&full_path)?;
                
                // Check for shebang
                if buffer.len() > 2 && buffer[0] == b'#' && buffer[1] == b'!' {
                    // Read first line to get interpreter
                    let mut first_line = Vec::new();
                    for &byte in buffer.iter().skip(2) {
                        if byte == b'\n' {
                            break;
                        }
                        first_line.push(byte);
                    }
                    
                    if let Ok(interpreter) = String::from_utf8(first_line) {
                        let interpreter = interpreter.trim();
                        // Split interpreter path and potential arguments
                        let parts: Vec<&str> = interpreter.split_whitespace().collect();
                        if !parts.is_empty() {
                            let interpreter_path = PathBuf::from(parts[0]);
                            // Recursively find the interpreter
                            return find_executable(&interpreter_path);
                        }
                    }
                }
                
                return Ok((full_path, buffer));
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Executable not found in PATH",
    ))
}
