//! The two behaviors iocc relies on: answer the manifest without the device and
//! refuse to run as a standalone program.

use std::process::Command;

fn plugin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iocc-mydev"))
}

#[test]
fn manifest_is_printed_without_iocc_and_without_the_device() {
    let output = plugin()
        .arg("--iocc-manifest")
        .env_remove("IOCC_HOST")
        .env_remove("IOCC_CONFIG")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let manifest: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest["protocol"], 1);
    assert!(!manifest["views"].as_array().unwrap().is_empty());
    assert!(!manifest["actions"].as_array().unwrap().is_empty());
}

#[test]
fn running_without_iocc_is_refused() {
    let output = plugin()
        .arg("status")
        .env_remove("IOCC_HOST")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("iocc mydev"), "{stderr}");
}
