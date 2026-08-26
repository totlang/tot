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
    2   something else went wrong

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

fn fmt(args: &[String]) -> Result<ExitCode, String> {
    let (flags, files) = split(args);
    let mut check_only = false;
    for flag in flags {
        match flag {
            "--check" => check_only = true,
            other => return Err(unknown_flag(other)),
        }
    }

    if files.is_empty() {
        let src = read_stdin()?;
        let formatted = tot::format(&src).map_err(|e| e.render(&src))?;
        if !check_only {
            print!("{formatted}");
            return Ok(ExitCode::SUCCESS);
        }
        if formatted == src {
            return Ok(ExitCode::SUCCESS);
        }
        eprintln!("tot: <stdin> is not formatted");
        return Ok(ExitCode::from(1));
    }

    let mut unformatted = false;
    for file in files {
        let src = read_file(file)?;
        let formatted = tot::format(&src).map_err(|e| format!("in {file}\n{}", e.render(&src)))?;
        if formatted == src {
            continue;
        }
        unformatted = true;
        if check_only {
            eprintln!("tot: {file} is not formatted");
        } else {
            std::fs::write(file, &formatted).map_err(|e| format!("{file}: {e}"))?;
        }
    }

    Ok(if check_only && unformatted {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn check(args: &[String]) -> Result<ExitCode, String> {
    let (flags, files) = split(args);
    if let Some(flag) = flags.first() {
        return Err(unknown_flag(flag));
    }

    let mut failed = false;
    if files.is_empty() {
        let src = read_stdin()?;
        if let Err(e) = tot::parse(&src) {
            eprint!("{}", e.render(&src));
            failed = true;
        }
    }
    for file in files {
        let src = read_file(file)?;
        if let Err(e) = tot::parse(&src) {
            eprintln!("tot: in {file}");
            eprint!("{}", e.render(&src));
            failed = true;
        }
    }

    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn to(args: &[String]) -> Result<ExitCode, String> {
    let (flags, positional) = split(args);
    let (format, file) = target(&positional, "to")?;

    let mut compact = false;
    let mut nulls = NullPolicy::Omit;
    for flag in flags {
        match flag {
            "--compact" => compact = true,
            "--null=omit" => nulls = NullPolicy::Omit,
            "--null=error" => nulls = NullPolicy::Error,
            other => return Err(unknown_flag(other)),
        }
    }

    let src = read_input(file)?;
    let value = tot::parse(&src).map_err(|e| e.render(&src))?;

    let out = match format {
        "json" => {
            if compact {
                tot::json::to_string(&value)
            } else {
                tot::json::to_string_pretty(&value)
            }
        }
        "yaml" => convert::to_yaml(&value)?,
        "toml" => {
            let (text, dropped) = convert::to_toml(&value, nulls)?;
            for path in &dropped {
                eprintln!("tot: dropped null at {path} — TOML has no null");
            }
            text
        }
        other => return Err(unknown_format(other)),
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
        "json" => tot::parse(&src).map_err(|e| e.render(&src))?,
        "yaml" => convert::from_yaml(&src)?,
        "toml" => {
            let (value, datetimes) = convert::from_toml(&src)?;
            for path in &datetimes {
                eprintln!("tot: datetime at {path} became a string — tot has no date type");
            }
            value
        }
        other => return Err(unknown_format(other)),
    };

    print!("{}", tot::format_value(&value));
    Ok(ExitCode::SUCCESS)
}

// --- argument plumbing --------------------------------------------------------------------

fn split(args: &[String]) -> (Vec<&str>, Vec<&str>) {
    args.iter()
        .map(String::as_str)
        .partition(|arg| arg.starts_with("--"))
}

fn target<'a>(positional: &[&'a str], command: &str) -> Result<(&'a str, Option<&'a str>), String> {
    let Some(format) = positional.first().copied() else {
        return Err(format!(
            "`tot {command}` needs a format: json, yaml, or toml"
        ));
    };
    if positional.len() > 2 {
        return Err(format!("`tot {command}` takes at most one file"));
    }
    Ok((format, positional.get(1).copied()))
}

fn unknown_flag(flag: &str) -> String {
    format!("unknown flag `{flag}` — try `tot help`")
}

fn unknown_format(format: &str) -> String {
    format!("unknown format `{format}` — expected json, yaml, or toml")
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
