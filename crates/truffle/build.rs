fn main() {
    // Try to get version from git tag
    let version = if let Ok(output) = std::process::Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
    {
        if output.status.success() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            // If git describe fails, try git rev-parse --short HEAD
            if let Ok(output) = std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
            {
                if output.status.success() {
                    format!("git-{}", String::from_utf8_lossy(&output.stdout).trim())
                } else {
                    env!("CARGO_PKG_VERSION").to_string()
                }
            } else {
                env!("CARGO_PKG_VERSION").to_string()
            }
        }
    } else {
        env!("CARGO_PKG_VERSION").to_string()
    };

    println!("cargo:rustc-env=TRUFFLE_VERSION={}", version);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
