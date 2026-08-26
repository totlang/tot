//! The `tot` command-line interface.

mod convert;

use std::io::Read;
use std::process::ExitCode;

use convert::NullPolicy;

const HELP: &str = "\
tot — JSON with the punctuation removed

USAGE
    tot fmt [--check] [FILE]...   format in place, or stdin to stdout
    tot check [--strict] [FILE]...
                                  parse and report errors
    tot merge [FILE]...           fold documents together, left to right
    tot get <PATH> [FILE]         print the one value at PATH
    tot set <PATH> <VALUE> [FILE] write VALUE at PATH and print the document
    tot to <FORMAT> [FILE]        write this document as json, yaml, or toml
    tot from <FORMAT> [FILE]      read json, yaml, or toml and write tot
    tot help

With no FILE, input is read from stdin. A FILE of `-` is stdin too, so it can be
one layer of a merge; a file actually named `-` is `./-`.

FLAGS
    --check         fmt: write nothing, and exit 1 if any file would change
    --strict        check: also warn when a member's value is not on its key's line
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
        print!("{HELP}");
        return Ok(ExitCode::SUCCESS);
    };
    match command {
        "fmt" => fmt(&args[1..]),
        "check" => check(&args[1..]),
        "merge" => merge(&args[1..]),
        "get" => get(&args[1..]),
        "set" => set(&args[1..]),
        "to" => to(&args[1..]),
        "from" => from(&args[1..]),
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(ExitCode::SUCCESS)
        }
        "--version" | "-V" => {
            println!("tot {}", env!("CARGO_PKG_VERSION"));
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
    for flag in flags {
        match flag {
            "--check" => check_only = true,
            other => return Err(unknown_flag(other)),
        }
    }

    let mut status = Status::default();
    for source in sources(&files) {
        let Some(src) = source.read(&mut status) else {
            continue;
        };
        let formatted = match tot::format(&src) {
            Ok(formatted) => formatted,
            Err(e) => {
                eprintln!("tot: in {}", source.label());
                eprint!("{}", e.render(&src));
                status.invalid();
                continue;
            }
        };

        if let (Source::Stdin, false) = (&source, check_only) {
            print!("{formatted}");
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
    for flag in flags {
        match flag {
            "--strict" => strict = true,
            other => return Err(unknown_flag(other)),
        }
    }

    let mut status = Status::default();
    for source in sources(&files) {
        let Some(src) = source.read(&mut status) else {
            continue;
        };
        // `lint` parses as well, so --strict costs no extra pass here.
        let result = if strict {
            tot::lint(&src)
        } else {
            tot::parse(&src).map(|_| Vec::new())
        };

        match result {
            Err(e) => {
                eprintln!("tot: in {}", source.label());
                eprint!("{}", e.render(&src));
                status.invalid();
            }
            Ok(warnings) if !warnings.is_empty() => {
                eprintln!("tot: in {}", source.label());
                for warning in &warnings {
                    eprint!("{}", warning.render(&src));
                }
                status.invalid();
            }
            Ok(_) => {}
        }
    }

    Ok(status.into())
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

    print!("{}", tot::format_value(&tot::merge(documents, nulls)));
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

    print!("{}", tot::format_value(&document));
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
        tot::Value::String(s) if raw => println!("{s}"),
        value => print!("{}", tot::format_value(value)),
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

    print!("{out}");
    // An empty document converts to empty text, and a lone newline is not a better rendering
    // of it than nothing at all.
    if !out.is_empty() && !out.ends_with('\n') {
        println!();
    }
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

    print!("{}", tot::format_value(&value));
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
