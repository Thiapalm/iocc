use std::fs;
use std::io::ErrorKind;
use std::process::Command;

// Must sort before 73-seat-late.rules for TAG+="uaccess" to take effect
const RULE_PATH: &str = "/etc/udev/rules.d/60-mcp2221.rules";
const OLD_RULE_PATH: &str = "/etc/udev/rules.d/99-mcp2221.rules";
const RULE: &str = r#"SUBSYSTEM=="usb", ATTRS{idVendor}=="04d8", ATTRS{idProduct}=="00dd", TAG+="uaccess"
KERNEL=="hidraw*", ATTRS{idVendor}=="04d8", ATTRS{idProduct}=="00dd", TAG+="uaccess"
"#;
// Content written by earlier versions at OLD_RULE_PATH; only a file identical to it is removed
const OLD_RULE: &str = r#"SUBSYSTEM=="usb", ATTRS{idVendor}=="04d8", ATTRS{idProduct}=="00dd", MODE="0666"
KERNEL=="hidraw*", ATTRS{idVendor}=="04d8", ATTRS{idProduct}=="00dd", MODE="0666"
"#;

/**
 * Installs the udev rule that lets the logged-in user (uaccess) access the USBIOExtender (MCP2221)
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

    // Rule written by earlier versions, would keep the device open to all users
    match fs::read_to_string(OLD_RULE_PATH) {
        Ok(content) if content == OLD_RULE => {
            if fs::remove_file(OLD_RULE_PATH).is_ok() {
                println!("Removed old rule {OLD_RULE_PATH}");
            }
        }
        Ok(_) => eprintln!(
            "Warning: {OLD_RULE_PATH} was not created by iocc and was kept; \
             remove it manually if it still grants broad access to the device"
        ),
        Err(_) => {}
    }

    run_udevadm(&["control", "--reload-rules"])?;
    run_udevadm(&[
        "trigger",
        "--subsystem-match=usb",
        "--subsystem-match=hidraw",
    ])?;
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
