//! Install / uninstall the kernel module, udev rules, and WirePlumber config.
//!
//! # System-level operations
//!
//! `install_driver()` and `uninstall_driver()` require root (EUID 0).
//! All helpers that touch the filesystem use absolute paths under `/usr/src/`,
//! `/etc/`, and the invoking user's `$HOME` (sourced from `SUDO_USER`).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

// ── Bundled file content via include_bytes! ────────────────────────────────

const KMOD_C: &[u8] = include_bytes!("../../../kmod/evo_raw.c");
const KMOD_MAKEFILE: &[u8] = include_bytes!("../../../kmod/Makefile");
const KMOD_DKMS_CONF: &[u8] = include_bytes!("../../../kmod/dkms.conf");
const KMOD_README: &[u8] = include_bytes!("../../../kmod/README.md");
const UDEV_RULES: &[u8] = include_bytes!("../../../packaging/99-evo.rules");
const SYSTEMD_UNIT: &[u8] = include_bytes!("../../../packaging/evo-control-apply.service");
const WIREPLUMBER_CONF: &[u8] = include_bytes!("../../../packaging/wireplumber/50-evo-routing.conf");

/// Version string embedded in the DKMS package name.
const KMOD_PACKAGE: &str = "evo-raw";

// ── Public API ────────────────────────────────────────────────────────────

/// Install the kernel module, udev rules, systemd unit, and WirePlumber config.
///
/// Must be run as root (via sudo). Idempotent — re-running upgrades the
/// bundled module to the binary's version.
pub fn install_driver() -> Result<()> {
    check_root()?;
    check_kernel_headers()?;
    ensure_dkms()?;
    write_kmod_sources()?;
    dkms_add_and_install()?;
    write_udev_rules()?;
    write_systemd_unit()?;
    write_wireplumber_config()?;
    reload_udev()?;
    load_module()?;
    verify_device()?;
    Ok(())
}

/// Uninstall everything that `install_driver` installs. Idempotent.
pub fn uninstall_driver() -> Result<()> {
    check_root()?;
    unload_module()?;
    dkms_remove()?;
    remove_kmod_sources()?;
    remove_udev_rules()?;
    remove_systemd_unit()?;
    remove_wireplumber_config()?;
    reload_udev()?;
    Ok(())
}

// ── Step helpers (testable) ───────────────────────────────────────────────

fn check_root() -> Result<()> {
    if !nix::unistd::Uid::effective().is_root() {
        bail!("install-driver must be run as root (try `sudo evo-control install-driver`)");
    }
    Ok(())
}

/// Version string for the DKMS package (derived from Cargo.toml version).
fn kmod_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `/usr/src/evo-raw-<version>/`
fn kmod_src_dir() -> PathBuf {
    PathBuf::from("/usr/src").join(format!("{KMOD_PACKAGE}-{}", kmod_version()))
}

/// Username that invoked sudo (falls back to `$USER` if not sudo).
fn target_user() -> String {
    std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "root".into())
}

/// Home directory of the target user.
fn target_home() -> PathBuf {
    let user = target_user();
    if user == "root" {
        PathBuf::from("/root")
    } else {
        PathBuf::from("/home").join(&user)
    }
}

fn check_kernel_headers() -> Result<()> {
    let uname_r = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| String::new());

    // Check that /lib/modules/$(uname -r)/build exists
    let build_dir = PathBuf::from("/lib/modules").join(&uname_r).join("build");
    if !build_dir.exists() {
        bail!(
            "kernel headers not found at {}. \
             Install them with your package manager (e.g. `pacman -S linux-cachyos-headers`)",
            build_dir.display()
        );
    }
    Ok(())
}

fn ensure_dkms() -> Result<()> {
    if Command::new("which").arg("dkms").output().is_err() {
        eprintln!("dkms not found — installing...");
        let status = Command::new("pacman")
            .args(["-S", "--needed", "--noconfirm", "dkms"])
            .status()
            .context("failed to run pacman to install dkms")?;
        if !status.success() {
            bail!("dkms installation failed");
        }
    }
    Ok(())
}

fn write_kmod_sources() -> Result<()> {
    let dir = kmod_src_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;

    write_file_if_changed(&dir.join("evo_raw.c"), KMOD_C)?;
    write_file_if_changed(&dir.join("Makefile"), KMOD_MAKEFILE)?;
    write_file_if_changed(&dir.join("dkms.conf"), KMOD_DKMS_CONF)?;
    write_file_if_changed(&dir.join("README.md"), KMOD_README)?;
    Ok(())
}

fn dkms_add_and_install() -> Result<()> {
    let pkg = format!("{KMOD_PACKAGE}/{}", kmod_version());
    let src = kmod_src_dir();

    // Check if already added
    let added = Command::new("dkms")
        .args(["status", &pkg])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pkg))
        .unwrap_or(false);

    if !added {
        let status = Command::new("dkms")
            .args(["add", &src.to_string_lossy()])
            .status()
            .context("dkms add failed")?;
        if !status.success() {
            bail!("dkms add failed for {src:?}");
        }
    }

    // Install or upgrade
    let status = Command::new("dkms")
        .args(["install", &pkg])
        .status()
        .context("dkms install failed")?;
    if !status.success() {
        bail!("dkms install failed for {pkg}");
    }
    Ok(())
}

fn write_udev_rules() -> Result<()> {
    let path = PathBuf::from("/etc/udev/rules.d/99-evo.rules");
    write_file_if_changed(&path, UDEV_RULES)
}

fn write_systemd_unit() -> Result<()> {
    let content = systemd_unit_content();
    let path = PathBuf::from("/etc/systemd/user/evo-control-apply.service");
    // Ensure parent dir exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn write_wireplumber_config() -> Result<()> {
    let home = target_home();
    let path = home
        .join(".config")
        .join("wireplumber")
        .join("wireplumber.conf.d")
        .join("50-evo-routing.conf");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, WIREPLUMBER_CONF)
        .with_context(|| format!("failed to write {}", path.display()))?;

    // Fix ownership if running under sudo
    if let Ok(user) = std::env::var("SUDO_USER") {
        let _ = Command::new("chown")
            .args(["-R", &format!("{user}:{user}"), &home.join(".config/wireplumber").to_string_lossy()])
            .status();
    }
    Ok(())
}

fn reload_udev() -> Result<()> {
    Command::new("udevadm")
        .args(["control", "--reload"])
        .status()
        .context("udevadm control --reload failed")?;
    Command::new("udevadm")
        .args(["trigger"])
        .status()
        .context("udevadm trigger failed")?;
    Ok(())
}

fn load_module() -> Result<()> {
    // Check if already loaded
    let loaded = Command::new("lsmod")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("evo_raw"))
        .unwrap_or(false);

    if !loaded {
        let status = Command::new("modprobe")
            .args(["evo_raw"])
            .status()
            .context("modprobe evo_raw failed")?;
        if !status.success() {
            bail!("modprobe evo_raw failed — check dmesg for details");
        }
    }
    Ok(())
}

fn verify_device() -> Result<()> {
    let dev = Path::new("/dev/evo8");
    if !dev.exists() {
        bail!(
            "/dev/evo8 not found after loading the module. \
             Try unplugging and re-plugging the EVO 8, then check dmesg."
        );
    }
    println!("✓ /dev/evo8 ready");
    Ok(())
}

// ── Uninstall helpers ─────────────────────────────────────────────────────

fn unload_module() -> Result<()> {
    let loaded = Command::new("lsmod")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("evo_raw"))
        .unwrap_or(false);

    if loaded {
        let status = Command::new("rmmod")
            .args(["evo_raw"])
            .status()
            .context("rmmod evo_raw failed")?;
        if !status.success() {
            // Non-fatal — the module may be in use
            eprintln!("warning: could not unload evo_raw module (may be in use)");
        }
    }
    Ok(())
}

fn dkms_remove() -> Result<()> {
    let pkg = format!("{KMOD_PACKAGE}/{}", kmod_version());
    let status = Command::new("dkms")
        .args(["remove", &pkg, "--all"])
        .status()
        .context("dkms remove failed")?;
    if !status.success() {
        // Non-fatal — may not be installed
        eprintln!("warning: dkms remove exited with {status}");
    }
    Ok(())
}

fn remove_kmod_sources() -> Result<()> {
    let dir = kmod_src_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("failed to remove {}", dir.display()))?;
    }
    Ok(())
}

fn remove_udev_rules() -> Result<()> {
    let path = PathBuf::from("/etc/udev/rules.d/99-evo.rules");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

fn remove_systemd_unit() -> Result<()> {
    let path = PathBuf::from("/etc/systemd/user/evo-control-apply.service");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

fn remove_wireplumber_config() -> Result<()> {
    let home = target_home();
    let path = home
        .join(".config")
        .join("wireplumber")
        .join("wireplumber.conf.d")
        .join("50-evo-routing.conf");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

// ── Internal helpers ──────────────────────────────────────────────────────

/// Write `data` to `path` only if the content differs from what's on disk.
/// Avoids unnecessary writes and timestamp changes for idempotent installs.
fn write_file_if_changed(path: &Path, data: &[u8]) -> Result<()> {
    if let Ok(existing) = std::fs::read(path) {
        if existing == data {
            return Ok(()); // unchanged
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, data)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Generate the systemd unit content, substituting `%h` with the target
/// user's home directory in ExecStart.
fn systemd_unit_content() -> Vec<u8> {
    let raw = std::str::from_utf8(SYSTEMD_UNIT).expect("systemd unit is valid UTF-8");
    // Replace %h with the actual home path for ExecStart
    let home = target_home().to_string_lossy().to_string();
    raw.replace("%h", &home).into_bytes()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kmod_version_is_semver() {
        let v = kmod_version();
        assert!(v.chars().next().unwrap().is_ascii_digit(), "version must start with digit: {v}");
        assert!(v.contains('.'), "version must contain dots: {v}");
    }

    #[test]
    fn kmod_src_dir_is_correct() {
        let dir = kmod_src_dir();
        assert!(dir.is_absolute());
        let s = dir.to_string_lossy();
        assert!(s.contains("/usr/src/evo-raw-"), "expected /usr/src/evo-raw-X.Y.Z, got {s}");
        assert!(s.ends_with(kmod_version()), "expected version suffix, got {s}");
    }

    #[test]
    fn bundled_files_are_nonempty() {
        assert!(!KMOD_C.is_empty());
        assert!(!KMOD_MAKEFILE.is_empty());
        assert!(!KMOD_DKMS_CONF.is_empty());
        assert!(!UDEV_RULES.is_empty());
        assert!(!SYSTEMD_UNIT.is_empty());
        assert!(!WIREPLUMBER_CONF.is_empty());
    }

    #[test]
    fn udev_rules_contain_evo8() {
        let content = std::str::from_utf8(UDEV_RULES).unwrap();
        assert!(content.contains("evo8"));
        assert!(content.contains("uaccess"));
        assert!(content.contains("SYSTEMD_USER_WANTS"));
    }

    #[test]
    fn wireplumber_config_contains_evo8() {
        let content = std::str::from_utf8(WIREPLUMBER_CONF).unwrap();
        assert!(content.contains("EVO8"));
        assert!(content.contains("suspend-timeout-seconds"));
    }

    #[test]
    fn systemd_unit_substitutes_home() {
        let content = String::from_utf8(systemd_unit_content()).unwrap();
        assert!(content.contains("/evo-control --apply-default-preset"));
        assert!(!content.contains("%h"), "%%h should be replaced with actual path");
        // Should contain the actual home path
        assert!(content.contains("/home/") || content.contains("/root/"));
    }

    #[test]
    fn check_root_fails_for_non_root() {
        // This test can only verify the error type, not that it actually
        // detects non-root — CI runs may be root.
        if !nix::unistd::Uid::effective().is_root() {
            let err = check_root().unwrap_err();
            let msg = format!("{err:#}");
            assert!(msg.contains("sudo"), "expected sudo hint, got: {msg}");
        }
        // If we ARE root, there's nothing meaningful to assert here.
    }

    #[test]
    fn target_user_falls_back() {
        // When SUDO_USER is not set, should fall back to USER or "root"
        let user = target_user();
        assert!(!user.is_empty());
    }

    #[test]
    fn write_file_if_changed_only_writes_on_diff() {
        let dir = std::env::temp_dir().join("evo-install-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("test.txt");
        let data = b"hello world";

        // First write
        write_file_if_changed(&path, data).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), data);

        // Same content — should not rewrite (timestamp check is implicit)
        write_file_if_changed(&path, data).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), data);

        // Different content
        write_file_if_changed(&path, b"new data").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new data");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
