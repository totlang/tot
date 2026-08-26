//! The `tot` command-line interface.

mod convert;

use std::io::Read;
use std::process::ExitCode;

use convert::NullPolicy;

const HELP: &str = "\
tot — JSON with the punctuation removed

USAGE
    tot fmt [--check] [FILE]...   format in place, or stdin to stdout
    tot check [FILE]...           parse and report errors
    tot to <FORMAT> [FILE]        write this document as json, yaml, or toml
    tot from <FORMAT> [FILE]      read json, yaml, or toml and write tot
    tot help

With no FILE, input is read from stdin.

FLAGS
    --check         fmt: write nothing, and exit 1 if any file would change
    --compact       to json: one line instead of indented
    --null=omit     to toml: drop null members and elements, reporting each (default)
    --null=error    to toml: refuse to convert instead

EXIT CODES
    0   success
    1   a file is unformatted, or a document failed to parse
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
    if let Some(flag) = flags.first() {
        return Err(unknown_flag(flag));
    }

    let mut status = Status::default();
    for source in sources(&files) {
        let Some(src) = source.read(&mut status) else {
            continue;
        };
        if let Err(e) = tot::parse(&src) {
            eprintln!("tot: in {}", source.label());
            eprint!("{}", e.render(&src));
            status.invalid();
        }
    }

    Ok(status.into())
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
    let value = tot::parse(&src).map_err(|e| e.render(&src))?;

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
    if !out.ends_with('\n') {
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
        Format::Json => tot::parse(&src).map_err(|e| e.render(&src))?,
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

/// What a command was pointed at. With no files named, that is stdin.
enum Source {
    Stdin,
    File(String),
}

impl Source {
    fn label(&self) -> &str {
        match self {
            Source::Stdin => "<stdin>",
            Source::File(path) => path,
        }
    }

    /// Reads the source, reporting and recording a failure rather than aborting the run.
    fn read(&self, status: &mut Status) -> Option<String> {
        let result = match self {
            Source::Stdin => read_stdin(),
            Source::File(path) => read_file(path),
        };
        match result {
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
    files
        .iter()
        .map(|path| Source::File((*path).to_string()))
        .collect()
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

fn split(args: &[String]) -> (Vec<&str>, Vec<&str>) {
    args.iter()
        .map(String::as_str)
        .partition(|arg| arg.starts_with("--"))
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
    match file {
        Some(path) => read_file(path),
        None => read_stdin(),
    }
}
