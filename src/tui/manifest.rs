use serde::Deserialize;
use serde_json::Value;

/// Flag a plugin must answer with a JSON manifest to describe itself to the TUI
pub const MANIFEST_FLAG: &str = "--iocc-manifest";

/**
 * Plugin self description, printed as JSON by `iocc-<name> --iocc-manifest`.
 * Plugins without a manifest still work, with a generic "run with arguments" entry.
 */
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub protocol: u32,
    pub title: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub views: Vec<View>,
    #[serde(default)]
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewKind {
    #[default]
    Text,
    Table,
    Metrics,
}

/// Read only panel: the command is run (periodically if `refresh_ms` is set) and its stdout rendered
#[derive(Debug, Clone, Deserialize)]
pub struct View {
    pub id: String,
    pub label: Option<String>,
    pub command: Vec<String>,
    #[serde(default)]
    pub kind: ViewKind,
    pub refresh_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgKind {
    #[default]
    Text,
    Number,
    Choice,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArgSpec {
    pub name: String,
    pub label: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: ArgKind,
    #[serde(default)]
    pub options: Vec<String>,
    pub default: Option<String>,
}

/// Command the user can trigger; `{name}` tokens in `command` are replaced by the argument values
#[derive(Debug, Clone, Deserialize)]
pub struct Action {
    pub id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub command: Vec<String>,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
    #[serde(default)]
    pub confirm: bool,
    /// Internal: split the first argument on whitespace (used by the generic entry)
    #[serde(skip)]
    pub free_form: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TableData {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub unit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetricsData {
    metrics: Vec<Metric>,
}

impl View {
    pub fn title(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.id)
    }
}

impl Action {
    pub fn title(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.id)
    }

    /**
     * Builds the argument list for the plugin from the collected values
     */
    pub fn build_args(&self, values: &[String]) -> Vec<String> {
        if self.free_form {
            return values
                .first()
                .map(|v| v.split_whitespace().map(String::from).collect())
                .unwrap_or_default();
        }
        self.command
            .iter()
            .map(|token| {
                let mut out = token.clone();
                for (spec, value) in self.args.iter().zip(values) {
                    out = out.replace(&format!("{{{}}}", spec.name), value);
                }
                out
            })
            .collect()
    }
}

impl ArgSpec {
    pub fn title(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }

    /**
     * Validates a value typed by the user
     */
    pub fn validate(&self, value: &str) -> Result<(), String> {
        match self.kind {
            ArgKind::Text => Ok(()),
            ArgKind::Number => value
                .parse::<f64>()
                .map(|_| ())
                .map_err(|_| format!("'{value}' is not a number")),
            ArgKind::Choice if self.options.iter().any(|o| o == value) => Ok(()),
            ArgKind::Choice => Err(format!("choose one of: {}", self.options.join(", "))),
        }
    }
}

impl Manifest {
    /**
     * Parses the plugin answer. Returns None when the output is not a valid manifest
     */
    pub fn parse(stdout: &str) -> Option<Manifest> {
        let manifest: Manifest = serde_json::from_str(stdout.trim()).ok()?;
        (manifest.protocol == 1).then_some(manifest)
    }

    /**
     * Manifest used for plugins that do not describe themselves
     */
    pub fn generic() -> Manifest {
        Manifest {
            protocol: 1,
            // Nothing is run automatically for plugins that did not describe themselves
            views: vec![],
            actions: vec![
                Action {
                    id: "help".into(),
                    label: Some("Show help".into()),
                    description: Some("Runs `iocc <plugin> --help`".into()),
                    command: vec!["--help".into()],
                    args: vec![],
                    confirm: false,
                    free_form: false,
                },
                Action {
                    id: "run".into(),
                    label: Some("Run with arguments".into()),
                    description: Some("Free form arguments, as typed after `iocc <plugin>`".into()),
                    command: vec![],
                    args: vec![ArgSpec {
                        name: "args".into(),
                        label: Some("Arguments".into()),
                        kind: ArgKind::Text,
                        options: vec![],
                        default: None,
                    }],
                    confirm: false,
                    free_form: true,
                },
            ],
            ..Manifest::default()
        }
    }
}

pub fn parse_table(stdout: &str) -> Result<TableData, String> {
    serde_json::from_str(stdout.trim()).map_err(|e| format!("invalid table output: {e}"))
}

pub fn parse_metrics(stdout: &str) -> Result<Vec<Metric>, String> {
    serde_json::from_str::<MetricsData>(stdout.trim())
        .map(|m| m.metrics)
        .map_err(|e| format!("invalid metrics output: {e}"))
}

/// Text shown in a table cell
pub fn cell_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "protocol": 1, "title": "Demo", "version": "0.1",
        "views": [{"id":"v","command":["status","--json"],"kind":"metrics","refresh_ms":500}],
        "actions": [{"id":"set","label":"Set","command":["set","--state","{state}"],
                     "args":[{"name":"state","type":"choice","options":["on","off"]}]}]
    }"#;

    #[test]
    fn parses_manifest() {
        let m = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(m.views[0].kind, ViewKind::Metrics);
        assert_eq!(m.views[0].refresh_ms, Some(500));
        assert_eq!(m.actions[0].args[0].kind, ArgKind::Choice);
    }

    #[test]
    fn rejects_other_output() {
        assert!(Manifest::parse("Usage: iocc-x [OPTIONS]").is_none());
        assert!(Manifest::parse(r#"{"protocol": 2}"#).is_none());
    }

    #[test]
    fn substitutes_args() {
        let m = Manifest::parse(SAMPLE).unwrap();
        let args = m.actions[0].build_args(&["on".into()]);
        assert_eq!(args, ["set", "--state", "on"]);
        assert!(m.actions[0].args[0].validate("on").is_ok());
        assert!(m.actions[0].args[0].validate("maybe").is_err());
    }

    #[test]
    fn generic_splits_free_form() {
        let m = Manifest::generic();
        let args = m.actions[1].build_args(&["read  --pin 3".into()]);
        assert_eq!(args, ["read", "--pin", "3"]);
    }

    #[test]
    fn parses_outputs() {
        let t = parse_table(r#"{"columns":["a"],"rows":[[1],["x"]]}"#).unwrap();
        assert_eq!(cell_text(&t.rows[0][0]), "1");
        let m = parse_metrics(r#"{"metrics":[{"name":"V","value":1.5,"unit":"V"}]}"#).unwrap();
        assert_eq!(m[0].value, 1.5);
    }
}
