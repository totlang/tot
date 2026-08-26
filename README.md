# tot

JSON with the punctuation removed. Whitespace-delimited `key value` pairs — no commas, no
colons, bare keys, quoted string values.

```tot
name "example-service"
version 3

listen {
  host "0.0.0.0"
  port 8080
}

regions ["us-west-2" "eu-central-1"]
```

[SPEC.md](SPEC.md) is the language definition and the source of truth. This file is how to use
it. [examples/config.tot](examples/config.tot) tours the syntax.

## Rules that will bite you

- **String values must be quoted.** `kind curly` is a parse error. Bare values are only
  numbers, `true`, `false`, `null`. Reason: two adjacent barewords are undecidable —
  `country united states` has no recoverable meaning.
- **Keys are bare unless they can't be.** Reserved outside strings: whitespace and
  `, : " { } [ ] # =`. Everything else is fine bare: `path/to/thing`,
  `com.example.key`, `my-name`, `123`. A dot is an ordinary character, not a nesting operator.
- **`,` and `:` are whitespace.** Every JSON file is valid tot, byte for byte. Trailing commas
  are legal.
- **`#` to end of line is the only comment.** No block form.
- **The top level is an object with no braces.** A document that is a single value *is* that
  value, so JSON scalar and array roots still parse.
- **Integers and floats are distinct types.** Float iff it has a `.` or an exponent, so
  `1 != 1.0` and `6e23` is a float. `1.` and `.1` are legal here and normalize to `1.0` and
  `0.1` on the way out to JSON.
- **No leading zeros.** `01234` is an error — zip codes have to be strings.
- **Duplicate keys are an error.** No last-wins.
- **No dates, anchors, tags, includes, or interpolation.** Not planned.

Multi-line strings:

```tot
motd """
  hello
  """
```

Indentation is stripped relative to the **closing** delimiter, so reindenting the block around
a string cannot change its value. No trailing newline unless you write `\n`.

## CLI

```bash
cargo build --release -p tot-cli    # target/release/tot
```

The root package is the library, so a bare `cargo build` won't produce the binary — name the
package, or use `--workspace`.

| | |
|---|---|
| `tot fmt [--check] [FILE]...` | format in place; `--check` writes nothing and exits 1 on a diff |
| `tot check [FILE]...` | parse and report diagnostics |
| `tot to <json\|yaml\|toml> [FILE]` | write this document as another format |
| `tot from <json\|yaml\|toml> [FILE]` | read another format and write tot |

No FILE means stdin. Every input is processed before exiting — one bad file doesn't hide the
rest. Exit codes: `0` ok, `1` unformatted or unparseable, `2` I/O or bad arguments.

Flags: `--compact` (`to json`), `--null=omit|error` (`to toml`, default `omit`). A flag that
doesn't apply to the chosen format is an error, not a no-op.

`from json` does no conversion. JSON is already tot, so it just reparses and reformats.

The formatter **preserves inline vs. block** and never reflows — running `fmt` over minified
JSON gives you back one correctly-punctuated long line, not an exploded document. It unquotes
keys where legal, indents two spaces, and emits LF.

### Interop caveats

| | |
|---|---|
| JSON | Lossless both ways. Comments dropped on the way out. |
| YAML | In: aliases resolved and inlined; tags, non-string keys, and multi-document streams rejected. Out: lossless. |
| TOML | In: datetimes become strings. Out: nulls dropped (`--null=error` refuses instead), root must be an object, sub-tables hoisted below plain values because TOML's syntax demands it. |

## Library

```rust
let value = tot::parse(src)?;                     // -> Value
let text  = tot::format(src)?;                    // canonical tot, source -> source
let text  = tot::format_value(&value);            // Value -> tot
let json  = tot::json::to_string_pretty(&value);
```

There is no JSON *parser* and there doesn't need to be — `parse` reads JSON directly.

Errors carry a span: `e.render(src)` gives a caret diagnostic.

Numbers keep their original lexeme, so integers wider than `i64` survive a round trip.
`Integer` equality is value equality, but **`Float` equality is lexical** — `1.0 != 1.00`.
Compare `as_f64()` if you mean value equality.

The `tot` crate has **no dependencies**. YAML and TOML need third-party parsers, so they live
in `tot-cli` instead.

## Layout

```
src/                tot — library, zero dependencies
  lex.rs            tokenizer
  parse.rs          recursive descent -> Value
  cst.rs            lossless tree: comments, blank lines, inline/block choice
  fmt.rs            formatter (over the CST) + Value -> tot emitter
  json.rs           JSON output only
  value.rs          Value, Integer, Float, Map
  error.rs          Span, Error, caret rendering
cli/                tot-cli — binary `tot`; deps: toml, yaml_serde
```

`parse` and `format` are separate walks over the same token stream. `format` validates with
`parse` first, then rebuilds a CST that keeps the trivia `parse` throws away — trivia is read
straight out of the gaps between token spans, so there's no second lexer.

## Build

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Edition 2024. Formatter tests assert two properties on every fixture, not just expected output:
formatting preserves the parsed value, and formatting is idempotent.

## Not built yet

- `tot check --strict` — warn when a member spans a newline
- `tot get <path>`
- serde `Serializer` / `Deserializer`
- `format_value` writes every string as a single-line escaped literal, so a newline-heavy
  string converted from JSON comes out as one long `\n`-laden line instead of a `"""` block
