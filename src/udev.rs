use std::fs;
use std::io::ErrorKind;
use std::process::Command;

const RULE_PATH: &str = "/etc/udev/rules.d/99-mcp2221.rules";
const RULE: &str = r#"SUBSYSTEM=="usb", ATTRS{idVendor}=="04d8", ATTRS{idProduct}=="00dd", MODE="0666"
KERNEL=="hidraw*", ATTRS{idVendor}=="04d8", ATTRS{idProduct}=="00dd", MODE="0666"
"#;

/**
 * Installs the udev rule that lets non-root users access the USBIOExtender (MCP2221)
 * and reloads udev. Requires root.
 */
pub fn setup_udev() -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("setup-udev is only supported on Linux".to_string());
    }

    fs::write(RULE_PATH, RULE).map_err(|e| match e.kind() {
        ErrorKind::PermissionDenied => {
            "permission denied, run with root privileges: sudo $(which iocc) setup-udev".to_string()
        }
        _ => format!("could not write {RULE_PATH}: {e}"),
    })?;
    println!("Rule installed at {RULE_PATH}");

    run_udevadm(&["control", "--reload-rules"])?;
    run_udevadm(&["trigger"])?;
    println!("udev reloaded. Unplug and replug the USBIOExtender board.");
    Ok(())
}

fn run_udevadm(args: &[&str]) -> Result<(), String> {
    let status = Command::new("udevadm")
        .args(args)
        .status()
        .map_err(|e| format!("could not run udevadm: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("udevadm {} failed ({status})", args.join(" ")))
    }
}
