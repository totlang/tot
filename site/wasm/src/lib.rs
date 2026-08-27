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
//! {"ok": true,  "value": "…", "warnings": [ … ]}
//! {"ok": false, "error": "…", "line": 4, "column": 13}
//! ```
//!
//! where `error` is the same caret diagnostic the CLI prints, because it is produced by the same
//! `render` call.

use std::collections::HashMap;

use tot::template::{Imports, Loaded};
use tot::{Dialect, Error, Map, Params, Schema, Template, Value};
use wasm_bindgen::prelude::wasm_bindgen;

// `convert.rs` is the CLI's, included rather than copied. A second copy of the YAML and TOML
// mappings would drift, and the playground exists to show what the tool actually does.
// `--null=error` is a CLI flag with no playground equivalent, so that variant is unused here.
#[allow(dead_code)]
#[path = "../../../cli/src/convert.rs"]
mod convert;

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
    emit(object(vec![
        ("ok", Value::Bool(true)),
        ("value", string(value)),
        ("warnings", Value::Array(warnings)),
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
/// is the CLI's default; anything it had to drop comes back in `dropped`.
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
                let mut notes = warnings;
                for path in dropped {
                    notes.push(object(vec![(
                        "render",
                        string(format!("note: dropped null at `{path}` — TOML has no null")),
                    )]));
                }
                ok(text, notes)
            }
            Err(message) => refused(message),
        },
        other => refused(format!("unknown target format `{other}`")),
    }
}

/// Reads one of the formats `tot from` knows and writes it as tot.
#[wasm_bindgen]
pub fn from_format(src: &str, source_format: &str) -> String {
    let value = match source_format {
        // JSON is already tot, so this is a reparse and a reformat rather than a conversion.
        "json" => match tot::parse(src) {
            Ok(value) => value,
            Err(error) => return failed(&error, src),
        },
        "yaml" => match convert::from_yaml(src) {
            Ok(value) => value,
            Err(message) => return refused(message),
        },
        "toml" => match convert::from_toml(src) {
            Ok((value, _datetimes)) => value,
            Err(message) => return refused(message),
        },
        other => return refused(format!("unknown source format `{other}`")),
    };
    ok(tot::format_value(&value), Vec::new())
}

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

// --- paths -------------------------------------------------------------------------------------

/// Reads one value out of a document, the way `tot get` does.
#[wasm_bindgen]
pub fn get(src: &str, path: &str, raw: bool) -> String {
    let value = match tot::parse(src) {
        Ok(value) => value,
        Err(error) => return failed(&error, src),
    };
    let parsed = match tot::Path::parse(path) {
        Ok(parsed) => parsed,
        // A malformed path indexes into the path, not the document, so it is reported on its own.
        Err(error) => return refused(error.render(path)),
    };
    match parsed.get(&value) {
        Ok(found) => {
            let text = match (raw, found) {
                (true, Value::String(s)) => s.clone(),
                _ => tot::format_value(found),
            };
            ok(text, Vec::new())
        }
        Err(missing) => refused(format!("no such path: {missing}")),
    }
}
