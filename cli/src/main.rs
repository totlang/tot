//! The `tot` command-line interface.

mod build;
mod convert;

use std::io::{Read, Write};
use std::process::ExitCode;

use convert::NullPolicy;

const HELP: &str = "\
tot — JSON with the punctuation removed

USAGE
    tot fmt [--check] [--template] [FILE]...
                                  format in place, or stdin to stdout
    tot check [--strict] [--template] [--schema=FILE] [FILE]...
                                  parse and report errors
    tot build [--check] [--out=FILE] [--set=N=V]... FILE
                                  build a .tott template into a .tot document
    tot merge [--null=…] [FILE]...
                                  fold documents together, left to right
    tot get [--raw] <PATH> [FILE] print the one value at PATH
    tot set [--raw] [--create] <PATH> <VALUE> [FILE]
                                  write VALUE at PATH and print the document
    tot to <FORMAT> [FILE]        write this document as json, yaml, or toml
    tot from <FORMAT> [FILE]      read json, yaml, or toml and write tot
    tot help                      this text; also --help or -h
    tot --version                 print the version; also -V

With no FILE, input is read from stdin. A FILE of `-` is stdin too, so it can be
one layer of a merge; a file actually named `-` is `./-`.

FLAGS
    --check         fmt: write nothing, and exit 1 if any file would change
                    build: exit 1 if the built document differs from the one on disk
    --template      fmt, check: read every input as a .tott template. A FILE's
                    extension already decides; this is for stdin, which has none
    --strict        check: also warn when a member's value is not on its key's line
    --schema=FILE   check: also check the shape against the schema in FILE
    --out=FILE      build: write here instead of stdout
    --set=N=V       build: set parameter N to the tot value V
    --set-raw=N=V   build: set parameter N to the string V, spelled literally
                    setting one name twice is an error, not a last-one-wins
    --raw           get: print a string with no quotes and no escapes
                    set: take VALUE as a string, spelled literally
    --create        set: build the objects on the way to PATH if they are missing
    --null=set      merge: an overlay's null sets the member to null (default)
    --null=delete   merge: an overlay's null removes the member instead
    --compact       to json: one line instead of indented
    --null=omit     to toml: drop null members and elements, reporting each (default)
    --null=error    to toml: refuse to convert instead
    --              end the flags; `-` is a bareword character, so a key, a path,
                    or a file may itself start with `--`

SCHEMA
    A schema is a tot document shaped like the ones it describes, with a type
    where each value would be:

        name    \"string\"
        listen  {host \"string\" port \"int\" tls? \"bool\"}
        regions [\"string\"]
        labels  {* \"string\"}
        retries \"int|null\"

    A type name is quoted because a schema is tot, and in tot a bare word is
    never a value. Types are any, string, int, float, bool, and null, joined
    with `|`. A `?` on a key makes the member optional; a `*` key covers every
    other key. Without one, a member the schema does not name is an error —
    catching a typo is most of what checking a shape is for.

TEMPLATES
    A .tott file is tot plus one thing: a `(head arg…)` form, wherever a value
    goes, evaluated at build time and replaced by its value.

        name     \"example-service\"
        replicas (if (param \"prod\") 5 1)
        image    (str \"registry/\" (param \"name\") \":\" (param \"tag\"))
        regions  (import \"regions.tot\")
        hosts    (map (str (it) \".example.net\") (import \"regions.tot\"))

    There are seven forms and no way to define an eighth:

        (param \"name\")           a build parameter, set with --set
        (param \"name\" default)   …or default, when it was not set
        (if cond then else)      cond must be a boolean; tot has no truthiness
        (str a b …)              joins strings, numbers, and booleans
        (import \"file\")          that file's value, read relative to this file
        (get path value)         the value at path inside value
        (get path value default) …or default, when there is nothing there
        (map body list)          body evaluated once per element of list
        (it)                     the element the enclosing map is on

    Parens are ordinary characters in .tot and delimiters only in .tott, so no
    .tot document changes meaning. Since parens never appear in data, anything
    inside them is computed and anything outside them is not.

    `get` reads out of a value you hand it and never out of the document being
    built, which would make a template mean something different depending on the
    order its members were evaluated in. Its third argument covers a member that
    is not there and an index past the end; a step into the wrong kind of value
    is a template bug and still fails. A `map` may not appear inside another
    map's body, so `(it)` names the element of exactly one of them and needs no
    shadowing rule; in the list argument it is fine.

    --out may name neither the template nor a fragment it imports: both are
    files being read, and there is nothing to recover either from.

    Parameters come from the command line and nowhere else, so a build is a pure
    function of its inputs. Write --set-raw=env=\"$ENV\" if you want the
    environment; that puts the dependency in plain sight.

    With no --out the document goes to stdout. --check builds and compares
    against the document on disk — config.tott against config.tot — so CI can
    catch one that has drifted from its template, the way fmt --check catches
    formatting.

MERGE
    Objects merge member by member; anything else is replaced whole. An array
    replaces rather than appending, because concatenation cannot be undone by a
    later layer.

SET
    VALUE is a tot value, spelled the way `get` prints one, so the two round-trip:
    `tot set port 8080`, `tot set tags '[\"a\" \"b\"]'`. Setting a string needs
    its quotes — `tot set name '\"svc\"'` — or `--raw`, which takes VALUE
    literally. The last step of PATH may be new; the ones before it must exist
    unless `--create` says otherwise. An array element is never created.

    `merge`, `get`, and `set` all write a document to stdout, so they chain.
    None of them keep comments: they fold values, the way `from` does.

PATHS
    `.` selects a member and `[n]` an element: `listen.port`, `regions[0].name`.
    A `.` is an ordinary character in a tot key, so a key holding one is quoted:
    `\"com.example\".level`. Output is tot, so it can be piped straight back in.

EXIT CODES
    0   success
    1   a file is unformatted, a document failed to parse, or a path was not found
    2   a file could not be read or written, or the command line was wrong

NOTE
    `from json` has no conversion step: every JSON document is already a valid tot
    document, so this only reformats it.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("tot: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let Some(command) = args.first().map(String::as_str) else {
        write_out(HELP)?;
        return Ok(ExitCode::SUCCESS);
    };
    match command {
        "fmt" => fmt(&args[1..]),
        "check" => check(&args[1..]),
        "build" => build_command(&args[1..]),
        "merge" => merge(&args[1..]),
        "get" => get(&args[1..]),
        "set" => set(&args[1..]),
        "to" => to(&args[1..]),
        "from" => from(&args[1..]),
        "help" | "--help" | "-h" => {
            write_out(HELP)?;
            Ok(ExitCode::SUCCESS)
        }
        "--version" | "-V" => {
            write_out(&format!("tot {}\n", env!("CARGO_PKG_VERSION")))?;
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command `{other}` — try `tot help`")),
    }
}

/// Formats every input it was given, and keeps going when one of them fails: a bad file in
/// the middle of a directory must not leave the rest unformatted and unreported.
fn fmt(args: &[String]) -> Result<ExitCode, String> {
    let (flags, files) = split(args);
    let mut check_only = false;
    let mut template = false;
    for flag in flags {
        match flag {
            "--check" => check_only = true,
            "--template" => template = true,
            other => return Err(unknown_flag(other)),
        }
    }

    let mut status = Status::default();
    for source in sources(&files) {
        let Some(src) = source.read(&mut status) else {
            continue;
        };
        let result = match source.dialect(template) {
            tot::Dialect::Template => tot::format_template(&src),
            tot::Dialect::Data => tot::format(&src),
        };
        let formatted = match result {
            Ok(formatted) => formatted,
            Err(e) => {
                eprintln!("tot: in {}", source.label());
                eprint!("{}", e.render(&src));
                status.invalid();
                continue;
            }
        };

        if let (Source::Stdin, false) = (&source, check_only) {
            write_out(&formatted)?;
            continue;
        }
        if formatted == src {
            continue;
        }
        match (&source, check_only) {
            // Rewriting a file is success; only --check treats a change as a failure.
            (_, true) => {
                status.invalid();
                eprintln!("tot: {} is not formatted", source.label());
            }
            (Source::File(path), false) => {
                if let Err(e) = std::fs::write(path, &formatted) {
                    eprintln!("tot: {path}: {e}");
                    status.broken();
                }
            }
            (Source::Stdin, false) => unreachable!("handled above"),
        }
    }

    Ok(status.into())
}

fn check(args: &[String]) -> Result<ExitCode, String> {
    let (flags, files) = split(args);
    let mut strict = false;
    let mut template = false;
    let mut schema_file = None;
    for flag in flags {
        match flag {
            "--strict" => strict = true,
            "--template" => template = true,
            _ if flag.starts_with("--schema=") => {
                schema_file = Some(&flag["--schema=".len()..]);
            }
            "--schema" => {
                return Err("`--schema` takes its file with an `=`: `--schema=shape.tot`".into());
            }
            other => return Err(unknown_flag(other)),
        }
    }

    // A schema that is not a schema is a bad command line, not a bad document.
    let schema = match schema_file {
        Some(path) => {
            let src = read_file(path)?;
            Some(tot::Schema::parse(&src).map_err(|e| format!("in {path}\n{}", e.render(&src)))?)
        }
        None => None,
    };

    let inputs = sources(&files);

    // A schema says what shape a document has, and a template does not have one until it is
    // built: `(param "x")` could be anything. Refusing beats checking the wrong thing, and the
    // pipeline that does work is worth naming.
    if schema.is_some()
        && let Some(source) = inputs
            .iter()
            .find(|source| source.dialect(template) == tot::Dialect::Template)
    {
        return Err(format!(
            "`--schema` checks a document, and {} is a template — build it first: \
             `tot build {} | tot check --schema=…`",
            source.label(),
            source.label()
        ));
    }

    let mut status = Status::default();
    for source in inputs {
        let Some(src) = source.read(&mut status) else {
            continue;
        };
        let dialect = source.dialect(template);
        let (warnings, violations) = match diagnose(&src, strict, schema.as_ref(), dialect) {
            Ok(found) => found,
            Err(e) => {
                eprintln!("tot: in {}", source.label());
                eprint!("{}", e.render(&src));
                status.invalid();
                continue;
            }
        };

        if warnings.is_empty() && violations.is_empty() {
            continue;
        }
        eprintln!("tot: in {}", source.label());
        for violation in &violations {
            eprint!("{}", violation.render(&src));
        }
        for warning in &warnings {
            eprint!("{}", warning.render(&src));
        }
        status.invalid();
    }

    Ok(status.into())
}

/// Builds a `.tott` template into a `.tot` document.
///
/// With no `--out`, the document goes to stdout, so a build composes with the rest of the CLI
/// the way `merge`, `get`, and `set` do. `--check` is the one that earns its keep in CI: it
/// builds and compares, so a committed document that has drifted from the template it came
/// from is caught the same way `fmt --check` catches formatting.
fn build_command(args: &[String]) -> Result<ExitCode, String> {
    let (flags, positional) = split(args);
    let mut check_only = false;
    let mut out: Option<&str> = None;
    let mut params = tot::Params::new();
    // Kept verbatim so that a diagnostic recommending a rebuild can recommend *this* build.
    // A suggestion that quietly drops them names a command that produces a different document.
    let mut given: Vec<&str> = Vec::new();

    for flag in flags {
        match flag {
            "--check" => check_only = true,
            _ if flag.starts_with("--out=") => out = Some(&flag["--out=".len()..]),
            _ if flag.starts_with("--set=") => {
                let (name, text) = pair(&flag["--set=".len()..], "--set")?;
                // Nothing at all is not the empty object, which is what parsing it would say.
                if text.is_empty() {
                    return Err(format!(
                        "`--set={name}=` has no value; write `--set-raw={name}=` for an \
                         empty string"
                    ));
                }
                // The same spelling `tot set` takes, so a value means one thing across the CLI.
                let value = tot::parse_value(text)
                    .map_err(|e| format!("`--set={name}=…`: `{text}` is not a tot value: {e}"))?;
                set_once(&params, name)?;
                params.set(name, value);
                given.push(flag);
            }
            _ if flag.starts_with("--set-raw=") => {
                let (name, text) = pair(&flag["--set-raw=".len()..], "--set-raw")?;
                set_once(&params, name)?;
                params.set(name, tot::Value::String(text.to_string()));
                given.push(flag);
            }
            // Each needs its `=` for the reason `--schema` does: bare, the thing after it
            // would be indistinguishable from the template to build.
            "--out" | "--set" | "--set-raw" => {
                return Err(format!("`{flag}` takes its value with an `=`: `{flag}=…`"));
            }
            other => return Err(unknown_flag(other)),
        }
    }

    let Some(input) = positional.first().copied() else {
        return Err("`tot build` needs a template, like `config.tott`".to_string());
    };
    if positional.len() > 1 {
        return Err("`tot build` takes one template".to_string());
    }

    let from_stdin = input == "-";
    let path = std::path::Path::new(input);

    // `build` reads its input as a template. A `.tot` file is a document — already built — and
    // reading one in the template dialect would report its parens as forms, which is a
    // confusing way to say "wrong file". Any other name is taken at its word: the extension
    // may be anything, and asking to build it is saying what it is.
    if !from_stdin && build::is_document(path) {
        return Err(format!(
            "`{input}` is a document, not a template — `tot build` turns a `.tott` file into \
             a `.tot` one"
        ));
    }

    // Writing over the file being read loses it, and there is nothing to recover it from.
    if let Some(out) = out
        && build::same_file(std::path::Path::new(out), path)
    {
        return Err(format!(
            "`--out={out}` is the template being built, and building over it would lose it"
        ));
    }

    // A template's imports resolve relative to itself, so it needs a name even when it came
    // from stdin — where the only sensible answer is the directory the build was run from.
    let name = if from_stdin {
        "<stdin>".to_string()
    } else {
        build::name(path)
    };
    let src = read_input(Some(input))?;

    let template = match tot::Template::parse_named(&src, &name) {
        Ok(template) => template,
        Err(e) => {
            eprintln!("tot: in {name}");
            eprint!("{}", e.render(&src));
            return Ok(ExitCode::from(1));
        }
    };
    let mut imports = build::Files::default();
    let document = match template.build(&params, &mut imports) {
        Ok(document) => document,
        Err(e) => {
            eprint!("tot: {}", e.render());
            return Ok(ExitCode::from(1));
        }
    };
    let built = tot::format_value(&document);

    if !check_only {
        // Without `--out` the document goes to stdout: writing a file nobody named would be a
        // surprise, and the inferred name exists only for `--check` to compare against.
        return match out {
            None => {
                write_out(&built)?;
                Ok(ExitCode::SUCCESS)
            }
            Some(out) => {
                // The template is refused up front, before anything is read. A fragment it
                // imports is only known once the build has run, but it is a file being read
                // just the same, and writing over one loses it with nothing to recover it from.
                let target = std::path::Path::new(out);
                if let Some(clash) = imports
                    .loaded()
                    .iter()
                    .find(|read| build::same_file(target, read))
                {
                    // The two spellings are usually the same one, and saying it twice reads
                    // like a mistake. Name the import only where it was reached differently.
                    let reached = clash.display().to_string();
                    let also = match reached == out {
                        true => String::new(),
                        false => format!(" (reached as `{reached}`)"),
                    };
                    return Err(format!(
                        "`--out={out}`{also} is a file this template imports, and building \
                         over it would lose it"
                    ));
                }
                match std::fs::write(out, &built) {
                    Ok(()) => Ok(ExitCode::SUCCESS),
                    Err(e) => Err(format!("{out}: {e}")),
                }
            }
        };
    }

    let target = match out {
        Some(out) => Some(std::path::PathBuf::from(out)),
        None if from_stdin => None,
        None => build::output_for(input),
    };
    let Some(path) = target else {
        // The inference is `.tott` → `.tot`, so it has nothing to work from when the template
        // came from stdin or was named something else.
        return Err(if from_stdin {
            "`tot build --check` needs `--out=FILE` when the template comes from stdin".to_string()
        } else {
            format!(
                "`tot build --check` needs `--out=FILE` for `{input}`: with no `--out` it \
                 compares against the template's own name without the `t`, and that only \
                 works for a `.tott` file"
            )
        });
    };
    let committed =
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if committed == built {
        return Ok(ExitCode::SUCCESS);
    }
    // The parameters go back into the suggestion. Without them it names a *different* build —
    // one that fails outright if a parameter has no default, or, worse, succeeds and writes a
    // document built from the defaults over the one that was just checked.
    let flags: String = given.iter().map(|flag| format!(" {flag}")).collect();
    eprintln!(
        "tot: {} is not what {name} builds — run `tot build{flags} --out={} {}`",
        path.display(),
        path.display(),
        input
    );
    Ok(ExitCode::from(1))
}

/// Refuses a parameter that was already set.
///
/// tot's most distinctive rule is that a duplicate key is an error rather than a race the last
/// writer wins, and a parameter set twice on one command line is the same question. Silently
/// keeping the last is how a shared script and a job override disagree without anyone noticing.
fn set_once(params: &tot::Params, name: &str) -> Result<(), String> {
    match params.get(name).is_some() {
        true => Err(format!(
            "parameter `{name}` was set twice — tot refuses a duplicate rather than \
             picking a winner"
        )),
        false => Ok(()),
    }
}

/// Splits a `name=value` flag argument.
fn pair<'a>(text: &'a str, flag: &str) -> Result<(&'a str, &'a str), String> {
    let (name, value) = text
        .split_once('=')
        .ok_or_else(|| format!("`{flag}={text}` needs a name and a value: `{flag}=name=value`"))?;
    if name.is_empty() {
        return Err(format!("`{flag}={text}` has no parameter name"));
    }
    Ok((name, value))
}

/// Runs the passes that were asked for over one document.
///
/// Everything here needs the document parsed, and `lint` and `Schema::check` each parse it
/// themselves — so whichever was requested is the parse, and a bare `parse` is the fallback
/// for when neither was. A document that does not parse fails at the first of them.
fn diagnose(
    src: &str,
    strict: bool,
    schema: Option<&tot::Schema>,
    dialect: tot::Dialect,
) -> Result<(Vec<tot::Warning>, Vec<tot::Violation>), tot::Error> {
    let template = dialect == tot::Dialect::Template;

    let warnings = match (strict, template) {
        (false, _) => Vec::new(),
        (true, true) => tot::lint_template(src)?,
        (true, false) => tot::lint(src)?,
    };
    let violations = match schema {
        // Refused above for a template, so this only ever sees a document.
        Some(schema) => schema.check(src)?,
        None => {
            if !strict {
                match template {
                    // Reading a template validates its forms too: an unknown head, a wrong
                    // argument count, a computed parameter name.
                    true => {
                        tot::Template::parse(src)?;
                    }
                    false => {
                        tot::parse(src)?;
                    }
                }
            }
            Vec::new()
        }
    };
    Ok((warnings, violations))
}

/// Folds documents together, left to right, and writes the result.
///
/// Unlike `fmt`, one bad input is the end of the run: there is a single output here, and a
/// document merged from only some of its layers is worse than no output at all.
fn merge(args: &[String]) -> Result<ExitCode, String> {
    let (flags, files) = split(args);
    let mut nulls = tot::Nulls::Set;
    for flag in flags {
        match flag {
            "--null=set" => nulls = tot::Nulls::Set,
            "--null=delete" => nulls = tot::Nulls::Delete,
            other => return Err(unknown_flag(other)),
        }
    }

    let mut documents = Vec::new();
    for source in sources(&files) {
        let src = source.read_now()?;
        let Some(value) = parse_or_report(&src, source.label()) else {
            return Ok(ExitCode::from(1));
        };
        documents.push(value);
    }

    write_out(&tot::format_value(&tot::merge(documents, nulls)))?;
    Ok(ExitCode::SUCCESS)
}

/// Writes one value into a document and prints the result.
///
/// The value argument is spelled the way `get` prints one, so the two round-trip: whatever
/// `tot get a` gives you is what `tot set a` takes back.
fn set(args: &[String]) -> Result<ExitCode, String> {
    let (flags, positional) = split(args);
    let mut raw = false;
    let mut missing = tot::Missing::Reject;
    for flag in flags {
        match flag {
            "--raw" => raw = true,
            "--create" => missing = tot::Missing::Create,
            other => return Err(unknown_flag(other)),
        }
    }

    let (Some(text), Some(literal)) = (positional.first().copied(), positional.get(1).copied())
    else {
        return Err("`tot set` needs a path and a value, like `listen.port 8080`".to_string());
    };
    if positional.len() > 3 {
        return Err("`tot set` takes at most one file".to_string());
    }
    let path = tot::Path::parse(text).map_err(|e| e.to_string())?;
    let value = if raw {
        tot::Value::String(literal.to_string())
    } else {
        // Nothing at all is not a tot value. Parsing it succeeds — an empty document is the
        // empty object — so without this, `tot set a ""` quietly writes `a {}`. This is the
        // same refusal `--set=N=` makes, and `--raw` is the same way out.
        if literal.is_empty() {
            return Err(format!(
                "`tot set {text}` was given an empty value; add `--raw` to set the empty string"
            ));
        }
        tot::parse_value(literal)
            .map_err(|e| format!("`{literal}` is not a valid tot value: {e}"))?
    };

    let file = positional.get(2).copied();
    let src = read_input(file)?;
    let Some(mut document) = parse_or_report(&src, label(file)) else {
        return Ok(ExitCode::from(1));
    };
    if let Err(e) = path.set(&mut document, value, missing) {
        eprintln!("tot: in {}: {e}", label(file));
        return Ok(ExitCode::from(1));
    }

    write_out(&tot::format_value(&document))?;
    Ok(ExitCode::SUCCESS)
}

/// Prints one value out of a document.
///
/// A path the document does not have is exit 1, not 2: the command line was fine, the document
/// simply did not answer it. A path that is not a path at all is the command line being wrong.
fn get(args: &[String]) -> Result<ExitCode, String> {
    let (flags, positional) = split(args);
    let mut raw = false;
    for flag in flags {
        match flag {
            "--raw" => raw = true,
            other => return Err(unknown_flag(other)),
        }
    }

    let Some(text) = positional.first().copied() else {
        return Err("`tot get` needs a path, like `listen.port`".to_string());
    };
    if positional.len() > 2 {
        return Err("`tot get` takes at most one file".to_string());
    }
    let path = tot::Path::parse(text).map_err(|e| e.to_string())?;

    let file = positional.get(1).copied();
    let src = read_input(file)?;
    let Some(document) = parse_or_report(&src, label(file)) else {
        return Ok(ExitCode::from(1));
    };
    let value = match path.get(&document) {
        Ok(value) => value,
        Err(e) => {
            // Rendered as one line: the span points into the path, so a caret under it would
            // sit beneath a line number that belongs to the document.
            eprintln!("tot: in {}: {e}", label(file));
            return Ok(ExitCode::from(1));
        }
    };

    match value {
        tot::Value::String(s) if raw => write_out(&format!("{s}\n"))?,
        value => write_out(&tot::format_value(value))?,
    }
    Ok(ExitCode::SUCCESS)
}

fn to(args: &[String]) -> Result<ExitCode, String> {
    let (flags, positional) = split(args);
    let (format, file) = target(&positional, "to")?;

    let mut compact = false;
    let mut nulls = NullPolicy::Omit;
    // Flags are checked against the format that was actually chosen, so one that cannot
    // apply is an error rather than a silent no-op.
    for flag in flags {
        match flag {
            "--compact" => {
                only_for(format, Format::Json, flag)?;
                compact = true;
            }
            "--null=omit" => {
                only_for(format, Format::Toml, flag)?;
                nulls = NullPolicy::Omit;
            }
            "--null=error" => {
                only_for(format, Format::Toml, flag)?;
                nulls = NullPolicy::Error;
            }
            other => return Err(unknown_flag(other)),
        }
    }

    let src = read_input(file)?;
    let Some(value) = parse_or_report(&src, label(file)) else {
        return Ok(ExitCode::from(1));
    };

    let out = match format {
        Format::Json => {
            if compact {
                tot::json::to_string(&value)
            } else {
                tot::json::to_string_pretty(&value)
            }
        }
        Format::Yaml => convert::to_yaml(&value)?,
        Format::Toml => {
            let (text, dropped) = convert::to_toml(&value, nulls)?;
            for path in &dropped {
                eprintln!("tot: dropped null at {path} — TOML has no null");
            }
            text
        }
    };

    // An empty document converts to empty text, and a lone newline is not a better rendering
    // of it than nothing at all.
    let terminated = match out.is_empty() || out.ends_with('\n') {
        true => out,
        false => out + "\n",
    };
    write_out(&terminated)?;
    Ok(ExitCode::SUCCESS)
}

fn from(args: &[String]) -> Result<ExitCode, String> {
    let (flags, positional) = split(args);
    if let Some(flag) = flags.first() {
        return Err(unknown_flag(flag));
    }
    let (format, file) = target(&positional, "from")?;
    let src = read_input(file)?;

    let value = match format {
        // Nothing to convert: JSON is tot.
        Format::Json => match parse_or_report(&src, label(file)) {
            Some(value) => value,
            None => return Ok(ExitCode::from(1)),
        },
        Format::Yaml => convert::from_yaml(&src)?,
        Format::Toml => {
            let (value, datetimes) = convert::from_toml(&src)?;
            for path in &datetimes {
                eprintln!("tot: datetime at {path} became a string — tot has no date type");
            }
            value
        }
    };

    write_out(&tot::format_value(&value))?;
    Ok(ExitCode::SUCCESS)
}

// --- inputs -------------------------------------------------------------------------------

/// What to call an input in a diagnostic.
fn label(file: Option<&str>) -> &str {
    match file {
        None | Some("-") => "<stdin>",
        Some(path) => path,
    }
}

/// Parses one document, reporting the diagnostic and answering `None` if it does not parse.
///
/// The commands that take a single input use this so that an unparseable document is exit 1
/// like everywhere else, rather than being reported as a bad command line.
fn parse_or_report(src: &str, label: &str) -> Option<tot::Value> {
    match tot::parse(src) {
        Ok(value) => Some(value),
        Err(e) => {
            eprintln!("tot: in {label}");
            eprint!("{}", e.render(src));
            None
        }
    }
}

/// What a command was pointed at. With no files named, that is stdin.
enum Source {
    Stdin,
    File(String),
}

impl Source {
    /// Resolves one name from the command line. `-` is stdin, as it is nearly everywhere; a
    /// file actually named `-` is `./-`.
    fn named(path: &str) -> Self {
        match path {
            "-" => Source::Stdin,
            path => Source::File(path.to_string()),
        }
    }

    fn label(&self) -> &str {
        match self {
            Source::Stdin => "<stdin>",
            Source::File(path) => path,
        }
    }

    /// Which language to read this input in.
    ///
    /// A file's extension decides, the same way it does for `(import …)`. Stdin has no
    /// extension and so is a document, unless `--template` says otherwise — guessing from the
    /// contents would be exactly the implicit typing the language exists to avoid.
    fn dialect(&self, force_template: bool) -> tot::Dialect {
        if force_template {
            return tot::Dialect::Template;
        }
        match self {
            Source::Stdin => tot::Dialect::Data,
            Source::File(path) => build::dialect(std::path::Path::new(path)),
        }
    }

    /// Reads the source, failing the whole command.
    fn read_now(&self) -> Result<String, String> {
        match self {
            Source::Stdin => read_stdin(),
            Source::File(path) => read_file(path),
        }
    }

    /// Reads the source, reporting and recording a failure rather than aborting the run.
    fn read(&self, status: &mut Status) -> Option<String> {
        match self.read_now() {
            Ok(src) => Some(src),
            Err(message) => {
                eprintln!("tot: {message}");
                status.broken();
                None
            }
        }
    }
}

fn sources(files: &[&str]) -> Vec<Source> {
    if files.is_empty() {
        return vec![Source::Stdin];
    }
    files.iter().copied().map(Source::named).collect()
}

/// The worst outcome seen so far, so that every input is processed before exiting.
#[derive(Default)]
struct Status(u8);

impl Status {
    /// A document is unformatted or failed to parse.
    fn invalid(&mut self) {
        self.0 = self.0.max(1);
    }

    /// A file could not be read or written.
    fn broken(&mut self) {
        self.0 = self.0.max(2);
    }
}

impl From<Status> for ExitCode {
    fn from(status: Status) -> Self {
        ExitCode::from(status.0)
    }
}

// --- argument plumbing --------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Json,
    Yaml,
    Toml,
}

impl Format {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "json" => Ok(Format::Json),
            "yaml" => Ok(Format::Yaml),
            "toml" => Ok(Format::Toml),
            other => Err(format!(
                "unknown format `{other}` — expected json, yaml, or toml"
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
        }
    }
}

/// Separates flags from positionals.
///
/// A bare `--` ends the flags: everything after it is positional, however it is spelled. That
/// matters here more than in most tools, because `-` is an ordinary bareword character, so
/// `--foo` is a legal key and a legal path — and a file may be named that way too.
fn split(args: &[String]) -> (Vec<&str>, Vec<&str>) {
    let mut flags = Vec::new();
    let mut positional = Vec::new();
    let mut rest = false;
    for arg in args.iter().map(String::as_str) {
        match arg {
            _ if rest => positional.push(arg),
            "--" => rest = true,
            _ if arg.starts_with("--") => flags.push(arg),
            _ => positional.push(arg),
        }
    }
    (flags, positional)
}

fn target<'a>(positional: &[&'a str], command: &str) -> Result<(Format, Option<&'a str>), String> {
    let Some(name) = positional.first().copied() else {
        return Err(format!(
            "`tot {command}` needs a format: json, yaml, or toml"
        ));
    };
    if positional.len() > 2 {
        return Err(format!("`tot {command}` takes at most one file"));
    }
    Ok((Format::parse(name)?, positional.get(1).copied()))
}

fn only_for(format: Format, wanted: Format, flag: &str) -> Result<(), String> {
    if format == wanted {
        return Ok(());
    }
    Err(format!(
        "`{flag}` applies only to `tot to {}`, not `{}`",
        wanted.name(),
        format.name()
    ))
}

fn unknown_flag(flag: &str) -> String {
    format!("unknown flag `{flag}` — try `tot help`")
}

/// Writes to stdout, treating a reader that has gone away as the ordinary end of a pipeline.
///
/// `print!` panics when the write fails, so `tot get big.tot | head -1` aborted with a Rust
/// panic and exit 101 — a code this tool's contract does not have, printed as a backtrace-style
/// message that reads like a crash. Every command here is meant to compose (`tot build c.tott |
/// tot check --schema=…`), and a downstream reader stopping early is what composing looks like.
///
/// Any other write failure is still a failure, and reaches exit 2 like the rest of them.
fn write_out(text: &str) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(format!("writing to stdout: {e}")),
    }
}

fn read_stdin() -> Result<String, String> {
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|e| format!("reading stdin: {e}"))?;
    Ok(buffer)
}

fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn read_input(file: Option<&str>) -> Result<String, String> {
    Source::named(file.unwrap_or("-")).read_now()
}
