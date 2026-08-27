//! The playground's bridge into the tot library.
//!
//! Every entry point takes strings and hands back a JSON string, because JSON is a language tot
//! already writes: results are assembled as a [`Value`] and serialised with `tot::json`, so this
//! crate needs no serialisation dependency of its own and cannot disagree with the parser about
//! how a string escapes.
//!
//! The shape is always one of:
//!
//! ```json
//! {"ok": true,  "value": "…", "warnings": [ … ], "notes": [ … ]}
//! {"ok": false, "error": "…", "line": 4, "column": 13}
//! ```
//!
//! where `error` is the same caret diagnostic the CLI prints, because it is produced by the same
//! `render` call. `warnings` and `notes` are separate for the same reason the CLI keeps them
//! apart: a warning is something wrong with the document and a note is something the conversion
//! did, and counting the second as the first makes a clean document look dirty.

use std::collections::HashMap;

use tot::template::{Imports, Loaded};
use tot::{Dialect, Error, Map, Params, Schema, Template, Value};
use wasm_bindgen::prelude::wasm_bindgen;

// `convert.rs` is the CLI's, included rather than copied. A second copy of the YAML and TOML
// mappings would drift, and the playground exists to show what the tool actually does. Only half
// of it is reachable from here: the playground writes the other formats but does not read them,
// and `--null=error` is a CLI flag with no equivalent on the page.
#[allow(dead_code)]
#[path = "../../../cli/src/convert.rs"]
mod convert;

// --- panics --------------------------------------------------------------------------------

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

/// Says what went wrong before the module dies.
///
/// A panic here traps, and the release profile aborts rather than unwinds, so the instance cannot
/// be used again: every later call fails too. The browser's own account of that is `unreachable
/// executed` and nothing else, which is not enough to reproduce anything. The hook runs before
/// the abort — the last moment the message still exists — so a bug that gets this far arrives
/// with a location on it.
#[wasm_bindgen(start)]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        console_error(&format!("tot-wasm panicked: {info}"));
    }));
}

// --- result plumbing ---------------------------------------------------------------------------

fn object(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn string(text: impl Into<String>) -> Value {
    Value::String(text.into())
}

fn integer(n: usize) -> Value {
    Value::Integer(tot::Integer::from_i64(n as i64))
}

fn emit(value: Value) -> String {
    tot::json::to_string(&value)
}

fn ok(value: String, warnings: Vec<Value>) -> String {
    reported(value, warnings, Vec::new())
}

/// A result that carries notes as well: things the conversion *did*, as against things wrong with
/// the document. `tot to toml` prints a dropped null as a note and still reports no warnings, so
/// the two travel in separate arrays and the page can count them the way the CLI does.
fn reported(value: String, warnings: Vec<Value>, notes: Vec<Value>) -> String {
    emit(object(vec![
        ("ok", Value::Bool(true)),
        ("value", string(value)),
        ("warnings", Value::Array(warnings)),
        ("notes", Value::Array(notes)),
    ]))
}

/// A parse failure, carrying the caret diagnostic and the position the editor should mark.
fn failed(error: &Error, src: &str) -> String {
    let (line, column) = error.line_col(src);
    emit(object(vec![
        ("ok", Value::Bool(false)),
        ("error", string(error.render(src))),
        ("line", integer(line)),
        ("column", integer(column)),
    ]))
}

/// A failure that has no span to point at — a converter refusing a document it cannot represent.
fn refused(message: String) -> String {
    emit(object(vec![
        ("ok", Value::Bool(false)),
        ("error", string(message)),
    ]))
}

fn warnings_for(src: &str, template: bool) -> Vec<Value> {
    let found = if template {
        tot::lint_template(src)
    } else {
        tot::lint(src)
    };
    found
        .unwrap_or_default()
        .into_iter()
        .map(|warning| {
            let (line, column) = warning.line_col(src);
            object(vec![
                ("render", string(warning.render(src))),
                ("line", integer(line)),
                ("column", integer(column)),
            ])
        })
        .collect()
}

// --- convert -----------------------------------------------------------------------------------

/// Reformats a document into canonical tot, keeping its comments and its inline/block choices.
///
/// `template` picks the dialect, the way the file extension does for the CLI: a `.tott` source
/// has forms and a `.tot` one has parens that are ordinary characters.
#[wasm_bindgen]
pub fn format(src: &str, template: bool) -> String {
    let formatted = if template {
        tot::format_template(src)
    } else {
        tot::format(src)
    };
    match formatted {
        Ok(text) => ok(text, warnings_for(src, template)),
        Err(error) => failed(&error, src),
    }
}

/// Writes a document as one of the formats `tot to` knows.
///
/// `target` is `tot`, `json`, `json-compact`, `yaml` or `toml`. A TOML target drops nulls, which
/// is the CLI's default; anything it had to drop comes back as a note.
#[wasm_bindgen]
pub fn convert(src: &str, target: &str) -> String {
    let value = match tot::parse(src) {
        Ok(value) => value,
        Err(error) => return failed(&error, src),
    };
    let warnings = warnings_for(src, false);

    match target {
        "tot" => match tot::format(src) {
            Ok(text) => ok(text, warnings),
            Err(error) => failed(&error, src),
        },
        "json" => ok(tot::json::to_string_pretty(&value), warnings),
        "json-compact" => ok(tot::json::to_string(&value), warnings),
        "yaml" => match convert::to_yaml(&value) {
            Ok(text) => ok(text, warnings),
            Err(message) => refused(message),
        },
        "toml" => match convert::to_toml(&value, convert::NullPolicy::Omit) {
            Ok((text, dropped)) => {
                let notes = dropped
                    .into_iter()
                    .map(|path| {
                        object(vec![(
                            "render",
                            string(format!("note: dropped null at `{path}` — TOML has no null")),
                        )])
                    })
                    .collect();
                reported(text, warnings, notes)
            }
            Err(message) => refused(message),
        },
        other => refused(format!("unknown target format `{other}`")),
    }
}

// There is no `from_format` here, and no `get`, though both were easy to write and the library
// has everything they need. An exported function is a root the linker cannot drop, so each one
// would pull its whole path into the download: `from_format` alone retains the TOML parser and
// libyaml's reading half for a direction the page has no control for. This crate is a page load,
// which is why the profile goes to the trouble it does — an export the playground never calls
// undoes that. Add them when the UI does, in the same commit.

// --- schema ------------------------------------------------------------------------------------

/// Checks a document against a schema, reporting every violation rather than the first.
///
/// A schema is itself tot, so a malformed one is reported the same way a malformed document is —
/// the `where` field says which of the two editors the diagnostic belongs to.
#[wasm_bindgen]
pub fn check_schema(document: &str, schema: &str) -> String {
    let parsed = match Schema::parse(schema) {
        Ok(parsed) => parsed,
        Err(error) => {
            let (line, column) = error.line_col(schema);
            return emit(object(vec![
                ("ok", Value::Bool(false)),
                ("where", string("schema")),
                ("error", string(error.render(schema))),
                ("line", integer(line)),
                ("column", integer(column)),
            ]));
        }
    };

    match parsed.check(document) {
        Ok(violations) => {
            let reported: Vec<Value> = violations
                .iter()
                .map(|violation| object(vec![("render", string(violation.render(document)))]))
                .collect();
            emit(object(vec![
                ("ok", Value::Bool(true)),
                ("violations", Value::Array(reported)),
                ("warnings", Value::Array(warnings_for(document, false))),
            ]))
        }
        Err(error) => {
            let (line, column) = error.line_col(document);
            emit(object(vec![
                ("ok", Value::Bool(false)),
                ("where", string("document")),
                ("error", string(error.render(document))),
                ("line", integer(line)),
                ("column", integer(column)),
            ]))
        }
    }
}

// --- templates ---------------------------------------------------------------------------------

/// `(import …)` resolved against a flat map of the playground's open files.
///
/// The playground has no directories, so a target names a file directly. Every load is recorded,
/// which is how the imports panel can say a file was parsed once however many times it was named.
struct OpenFiles {
    files: HashMap<String, String>,
    loaded: Vec<String>,
}

impl Imports for OpenFiles {
    fn load(&mut self, _from: &str, target: &str) -> Result<Loaded, String> {
        let source = self
            .files
            .get(target)
            .ok_or_else(|| format!("cannot import `{target}`: no such file is open"))?;
        self.loaded.push(target.to_string());
        Ok(Loaded {
            name: target.to_string(),
            source: source.clone(),
            // The extension picks the dialect here exactly as it does on disk: a `.tot` file is
            // data even when a template imported it.
            dialect: if target.to_ascii_lowercase().ends_with(".tott") {
                Dialect::Template
            } else {
                Dialect::Data
            },
        })
    }
}

/// Builds a template into a document.
///
/// `files` is a JSON object of filename to source, `entry` names the one to build, and `params`
/// is a JSON array of `{"name", "value", "raw"}`. A raw parameter is taken as a literal string,
/// the way `--set-raw` does; anything else is parsed as a tot value, the way `--set` does.
#[wasm_bindgen]
pub fn build(files: &str, entry: &str, params: &str) -> String {
    let open = match parse_files(files) {
        Ok(open) => open,
        Err(message) => return refused(message),
    };
    let parameters = match parse_params(params) {
        Ok(parameters) => parameters,
        Err(message) => return refused(message),
    };

    let Some(source) = open.get(entry) else {
        return refused(format!("no file called `{entry}` is open"));
    };
    let source = source.clone();

    let template = match Template::parse_named(&source, entry) {
        Ok(template) => template,
        Err(error) => return failed(&error, &source),
    };

    let mut imports = OpenFiles {
        files: open,
        loaded: Vec::new(),
    };

    match template.build(&parameters, &mut imports) {
        Ok(value) => {
            // How many times each file was *reached*, which is not how many times it was built:
            // the evaluator caches by name, so a file named three times is built once. Reporting
            // a measured count beats repeating the guarantee and hoping.
            let mut order: Vec<String> = Vec::new();
            let mut reads: HashMap<String, usize> = HashMap::new();
            for name in &imports.loaded {
                if !reads.contains_key(name) {
                    order.push(name.clone());
                }
                *reads.entry(name.clone()).or_insert(0) += 1;
            }
            let listed: Vec<Value> = order
                .iter()
                .map(|name| {
                    let size = imports.files.get(name).map(String::len).unwrap_or(0);
                    object(vec![
                        ("name", string(name.clone())),
                        ("bytes", integer(size)),
                        ("reads", integer(reads[name])),
                    ])
                })
                .collect();
            emit(object(vec![
                ("ok", Value::Bool(true)),
                ("value", string(tot::format_value(&value))),
                ("imports", Value::Array(listed)),
                ("warnings", Value::Array(warnings_for(&source, true))),
            ]))
        }
        // A build failure already knows which file it happened in and how the build reached it,
        // so it renders itself rather than being handed a source to point into.
        Err(failure) => emit(object(vec![
            ("ok", Value::Bool(false)),
            ("error", string(failure.render())),
            ("file", string(failure.file().to_string())),
            (
                "chain",
                Value::Array(failure.chain().iter().map(|f| string(f.clone())).collect()),
            ),
        ])),
    }
}

fn parse_files(json: &str) -> Result<HashMap<String, String>, String> {
    let value = tot::parse(json).map_err(|e| format!("bad files argument: {e}"))?;
    let Value::Object(map) = value else {
        return Err("files must be an object of filename to source".to_string());
    };
    let mut files = HashMap::new();
    for (name, source) in map.iter() {
        let Value::String(text) = source else {
            return Err(format!("file `{name}` is not a string"));
        };
        files.insert(name.to_string(), text.clone());
    }
    Ok(files)
}

fn parse_params(json: &str) -> Result<Params, String> {
    let value = tot::parse(json).map_err(|e| format!("bad params argument: {e}"))?;
    let Value::Array(entries) = value else {
        return Err("params must be an array".to_string());
    };

    let mut params = Params::new();
    for entry in entries {
        let Value::Object(fields) = entry else {
            return Err("each param must be an object".to_string());
        };
        let Some(Value::String(name)) = fields.get("name") else {
            return Err("each param needs a string `name`".to_string());
        };
        let Some(Value::String(text)) = fields.get("value") else {
            return Err(format!("param `{name}` needs a string `value`"));
        };
        let raw = matches!(fields.get("raw"), Some(Value::Bool(true)));

        // The CLI refuses a name set twice rather than picking a winner, and its help says so in
        // as many words. The params panel puts a duplicate one click away, so refusing it here is
        // what keeps the page from teaching the opposite of the rule it is demonstrating.
        if params.get(name).is_some() {
            return Err(format!(
                "parameter `{name}` was set twice — tot refuses a duplicate rather than \
                 picking a winner"
            ));
        }

        let value = if raw {
            Value::String(text.clone())
        } else {
            // The same grammar as a document, and the same diagnostic difference: a bare word
            // here is a string that forgot its quotes, not a key that lost its value.
            tot::parse_value(text)
                .map_err(|e| format!("`--set={name}={text}` is not a tot value: {e}"))?
        };
        params.set(name.clone(), value);
    }
    Ok(params)
}

// --- tests ---------------------------------------------------------------------------------

// These run on the host, not in a browser: the crate is an `rlib` as well as a `cdylib`, and a
// `#[wasm_bindgen]` function compiles to an ordinary one off wasm32. What they are for is the
// seam — `convert.rs` arrives here by `#[path]`, and the whole reason it does is that the page
// must not be able to disagree with `tot to`. Nothing else in the repository compiles this file.
#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a field back out of a result. The bridge answers in JSON, which is a language tot
    /// parses, so the tests read the answers with the same parser that wrote them.
    fn field(json: &str, path: &str) -> Value {
        let parsed = tot::parse(json).expect("the bridge emits JSON");
        tot::Path::parse(path)
            .expect("a literal path")
            .get(&parsed)
            .unwrap_or_else(|e| panic!("no `{path}` in {json}: {e}"))
            .clone()
    }

    fn count(json: &str, path: &str) -> usize {
        match field(json, path) {
            Value::Array(items) => items.len(),
            other => panic!("`{path}` is {other:?}, not an array"),
        }
    }

    fn is_ok(json: &str) -> bool {
        field(json, "ok") == Value::Bool(true)
    }

    #[test]
    fn a_document_converts_to_every_target_the_page_offers() {
        for target in ["tot", "json", "json-compact", "yaml", "toml"] {
            let out = convert("a 1 b \"two\"", target);
            assert!(is_ok(&out), "{target} failed: {out}");
        }
    }

    #[test]
    fn an_unknown_target_is_refused_rather_than_guessed() {
        let out = convert("a 1", "xml");
        assert!(!is_ok(&out));
        assert_eq!(
            field(&out, "error"),
            Value::String("unknown target format `xml`".into())
        );
    }

    #[test]
    fn a_parse_error_carries_the_caret_and_a_place_to_put_it() {
        // A bare word in value position: the mistake the language exists to make loud.
        let out = format("kind curly", false);
        assert!(!is_ok(&out));
        let Value::String(rendered) = field(&out, "error") else {
            panic!("the error is a string");
        };
        assert!(rendered.contains('^'), "no caret in {rendered}");
        assert_eq!(field(&out, "line"), integer(1));
    }

    #[test]
    fn a_dropped_null_is_a_note_and_not_a_warning() {
        // TOML has no null, so `tot to toml` drops one and says so. It is not a complaint about
        // the document, and counting it as one would make a clean document look dirty.
        let out = convert("a 1 retries null", "toml");
        assert!(is_ok(&out), "{out}");
        assert_eq!(count(&out, "notes"), 1, "{out}");
        assert_eq!(count(&out, "warnings"), 0, "{out}");
    }

    #[test]
    fn the_strict_lint_reaches_the_page() {
        // A member split across a line is what `tot check --strict` objects to.
        assert_eq!(count(&format("timeout\n30", false), "warnings"), 1);
        assert_eq!(count(&format("timeout 30", false), "warnings"), 0);
    }

    #[test]
    fn a_parameter_set_twice_is_refused_the_way_the_cli_refuses_it() {
        let files = r#"{"a.tott": "tag (param \"tag\")"}"#;
        let twice = r#"[{"name":"tag","value":"\"x\"","raw":false},
                        {"name":"tag","value":"\"y\"","raw":false}]"#;
        let out = build(files, "a.tott", twice);
        assert!(!is_ok(&out), "a duplicate was accepted: {out}");
        let Value::String(message) = field(&out, "error") else {
            panic!("the error is a string");
        };
        assert!(message.contains("was set twice"), "{message}");
    }

    #[test]
    fn a_raw_parameter_is_a_literal_string_and_a_set_one_is_parsed() {
        let files = r#"{"a.tott": "v (param \"v\")"}"#;
        let raw = build(files, "a.tott", r#"[{"name":"v","value":"1","raw":true}]"#);
        assert_eq!(field(&raw, "value"), Value::String("v \"1\"\n".into()));

        let set = build(files, "a.tott", r#"[{"name":"v","value":"1","raw":false}]"#);
        assert_eq!(field(&set, "value"), Value::String("v 1\n".into()));
    }

    #[test]
    fn a_template_builds_and_says_what_it_imported() {
        let files = r#"{"a.tott": "regions (import \"r.tot\")", "r.tot": "[\"us\" \"eu\"]"}"#;
        let out = build(files, "a.tott", "[]");
        assert!(is_ok(&out), "{out}");
        assert_eq!(count(&out, "imports"), 1, "{out}");
        // A built document has no CST behind it, so it comes back in canonical block form rather
        // than keeping the inline shape the imported file was written in.
        assert_eq!(
            field(&out, "value"),
            Value::String("regions [\n  \"us\"\n  \"eu\"\n]\n".into())
        );
    }

    #[test]
    fn an_import_of_a_file_that_is_not_open_names_the_file() {
        let files = r#"{"a.tott": "x (import \"gone.tot\")"}"#;
        let out = build(files, "a.tott", "[]");
        assert!(!is_ok(&out));
        let Value::String(message) = field(&out, "error") else {
            panic!("the error is a string");
        };
        assert!(message.contains("gone.tot"), "{message}");
    }

    #[test]
    fn a_schema_and_a_document_are_told_apart_when_one_of_them_will_not_parse() {
        let bad_schema = check_schema("a 1", "port int");
        assert!(!is_ok(&bad_schema));
        assert_eq!(field(&bad_schema, "where"), Value::String("schema".into()));

        let bad_document = check_schema("port 8080 port 80", r#"port "int""#);
        assert!(!is_ok(&bad_document));
        assert_eq!(
            field(&bad_document, "where"),
            Value::String("document".into())
        );
    }

    #[test]
    fn every_violation_is_reported_and_not_just_the_first() {
        let out = check_schema(r#"port "80" host 1"#, r#"port "int" host "string""#);
        assert!(is_ok(&out), "{out}");
        assert_eq!(count(&out, "violations"), 2, "{out}");
    }
}
