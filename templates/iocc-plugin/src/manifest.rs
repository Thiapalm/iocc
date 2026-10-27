//! JSON manifest answered to `iocc-<name> --iocc-manifest`.
//!
//! The TUI calls this before anything else and without opening the device. The
//! action defaults come from the saved settings, so the prompts open with the
//! value in use and the next run starts where the previous one stopped.

use crate::config::Settings;
use crate::device::{GAINS, Reading};

/// Flag the plugin answers to describe itself to the iocc TUI
pub const MANIFEST_FLAG: &str = "--iocc-manifest";

/// Panels and actions shown by `iocc tui` on the Plugins tab.
///
/// - `live` and `voltage` keep sampling through the TUI broker (`shared`), so
///   they survive while other plugins use the bus;
/// - `status` is a snapshot table refreshed every 5 s;
/// - `guide` is a one-shot plain text panel.
///
/// `set gain` and `set threshold` are `shared` because they touch the bus but
/// leave it usable. `reset` keeps the default (`exclusive`): it invalidates the
/// device state, so the TUI pauses the streams while it runs.
pub fn json(settings: &Settings) -> String {
    // The broker only exists on Unix; elsewhere every view/action must let the
    // TUI release the MCP2221 before running.
    let device = if cfg!(unix) { "shared" } else { "exclusive" };
    serde_json::json!({
        "protocol": 1,
        "title": "My device (template)",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Starter plugin: metrics, a chart, a table and settings saved in config.yaml",
        "views": [
            {"id": "live", "label": "Live readings", "kind": "metrics", "stream": true,
             "device": device, "command": ["watch", "--json", "--interval-ms", "200"]},
            {"id": "voltage", "label": "Voltage graph", "kind": "chart", "metric": "Voltage",
             "stream": true, "device": device, "command": ["watch", "--json", "--interval-ms", "500"]},
            {"id": "status", "label": "Status", "kind": "table", "device": device,
             "refresh_ms": 5000, "command": ["status", "--json"]},
            {"id": "guide", "label": "What to try", "kind": "text", "command": ["guide"]}
        ],
        "actions": [
            {"id": "gain", "label": "Set gain",
             "description": "change the amplifier gain and save it in config.yaml",
             "args": [{"name": "gain", "label": "Gain", "type": "choice",
                       "options": GAINS.map(|gain| gain.to_string()),
                       "default": settings.gain.to_string()}],
             "device": device, "command": ["set", "gain", "{gain}"]},
            {"id": "threshold", "label": "Set alert threshold",
             "description": "change the threshold in volts and save it in config.yaml",
             "args": [{"name": "voltage", "label": "Threshold (V)", "type": "number",
                       "default": format!("{}", settings.threshold)}],
             "device": device, "command": ["set", "threshold", "{voltage}"]},
            {"id": "reset", "label": "Reset device", "confirm": true, "command": ["reset"]}
        ]
    })
    .to_string()
}

/// JSON table for the `status --json` view
pub fn status_table(
    address: &str,
    transport: &str,
    chip: u16,
    gain: u8,
    threshold: f64,
    bus_info: &str,
) -> String {
    let rows = vec![
        vec!["Address".to_string(), address.to_string()],
        vec!["Transport".to_string(), transport.to_string()],
        vec!["Chip ID".to_string(), format!("0x{chip:04X}")],
        vec!["Gain".to_string(), format!("x{gain}")],
        vec!["Threshold".to_string(), format!("{threshold:.3} V")],
        vec!["Bus".to_string(), bus_info.replace('\n', " ")],
    ];
    serde_json::json!({
        "columns": ["Field", "Value"],
        "rows": rows,
    })
    .to_string()
}

/// JSON metrics for the `read --json` and `watch --json` views
pub fn metrics(reading: &Reading) -> String {
    serde_json::json!({
        "metrics": [
            {"name": "Voltage", "value": reading.voltage, "unit": "V"},
            {"name": "Current", "value": reading.current, "unit": "A"},
            {"name": "Temperature", "value": reading.temperature, "unit": "°C"},
        ]
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::CHIP_ID;

    #[test]
    fn manifest_is_valid_for_the_tui_protocol() {
        let value: serde_json::Value = serde_json::from_str(&json(&Settings::default())).unwrap();
        assert_eq!(value["protocol"], 1);
        assert_eq!(value["views"].as_array().unwrap().len(), 4);
        assert_eq!(value["actions"].as_array().unwrap().len(), 3);

        let chart = value["views"]
            .as_array()
            .unwrap()
            .iter()
            .find(|view| view["id"] == "voltage")
            .unwrap();
        assert_eq!(chart["kind"], "chart");
        assert_eq!(chart["metric"], "Voltage");
        let expected = if cfg!(unix) { "shared" } else { "exclusive" };
        assert_eq!(chart["device"], expected);

        let live = value["views"]
            .as_array()
            .unwrap()
            .iter()
            .find(|view| view["id"] == "live")
            .unwrap();
        assert_eq!(live["kind"], "metrics");
        assert_eq!(live["stream"].as_bool(), Some(true));
        assert_eq!(live["device"], expected);
    }

    #[test]
    fn action_defaults_follow_the_saved_configuration() {
        let settings = Settings {
            gain: 4,
            threshold: 1.5,
        };
        let value: serde_json::Value = serde_json::from_str(&json(&settings)).unwrap();
        assert_eq!(value["actions"][0]["args"][0]["default"], "4");
        assert_eq!(value["actions"][1]["args"][0]["default"], "1.5");
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn status_and_metrics_parse() {
        let table: serde_json::Value = serde_json::from_str(&status_table(
            "0x4A",
            "direct",
            CHIP_ID,
            1,
            2.0,
            "MCP rev A\nmore",
        ))
        .unwrap();
        assert_eq!(table["columns"][0], "Field");
        assert_eq!(table["rows"][2][1], "0x4D59");
        assert_eq!(table["rows"][5][1], "MCP rev A more");

        let reading = Reading {
            voltage: 12.0,
            current: 1.0,
            temperature: 25.5,
        };
        let metrics: serde_json::Value = serde_json::from_str(&metrics(&reading)).unwrap();
        assert_eq!(metrics["metrics"].as_array().unwrap().len(), 3);
        assert!(
            metrics["metrics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|metric| metric["name"] == "Voltage")
        );
    }
}
