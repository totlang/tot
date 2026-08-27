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
- **Keys are bare unless they can't be.** Reserved outside strings: whitespace, control
  characters, and `, : " { } [ ] # =` — plus `(` and `)`, but only in a `.tott` template.
  Everything else is fine bare: `path/to/thing`, `com.example.key`, `my-name`, `123`. A dot is
  an ordinary character, not a nesting operator. A key that holds a reserved character is
  written quoted, escapes and all: `"a\u0001b"`.
- **`,` and `:` are whitespace.** Every JSON file is valid tot, byte for byte. Trailing commas
  are legal.
- **`#` to end of line is the only comment.** No block form.
- **The top level is an object with no braces.** A document that is a single value *is* that
  value, so JSON scalar and array roots still parse.
- **Integers and floats are distinct types.** Float iff it has a `.` or an exponent, so
  `1 != 1.0` and `6e23` is a float. `1.` and `.1` are legal here and normalize to `1.0` and
  `0.1` on the way out to JSON.
- **No leading zeros.** `01234` is an error — zip codes have to be strings.
- **Integers have no range limit; floats do.** An integer keeps its lexeme, so a 400-digit one
  survives. A float has to denote a real `f64`, so `1e999` is a parse error — tot has no way
  to write an infinity. (`1e-999` is fine; it's zero.)
- **Duplicate keys are an error.** No last-wins.
- **No dates, anchors, tags, includes, or interpolation.** Dates, anchors, and tags aren't
  planned. Composing documents is — see [Composition](SPEC.md#composition-prospective) — but
  the point of that design is to serve it from the CLI and a separate template file, so the
  data language keeps this rule.

Multi-line strings:

```tot
motd """
  hello
  """
```

Indentation is stripped relative to the **closing** delimiter, so reindenting the block around
a string cannot change its value. No trailing newline unless you write `\n`.

The closing `"""` **owns its whole line** — nothing may follow it. Write `\"""` for a literal
`"""` opening a content line; otherwise it would close the string early and the error would
land somewhere further down.

## CLI

```bash
cargo install --path cli            # puts `tot` on your PATH
```

Or build it in place:

```bash
cargo build --release -p tot-cli    # target/release/tot
```

The root package is the library, so a bare `cargo build` won't produce the binary — name the
package, or use `--workspace`. Rust 1.88 or newer, which is what edition 2024 plus let-chains
needs; `rust-version` says so in both manifests, so cargo will tell you rather than the
compiler.

| | |
|---|---|
| `tot fmt [--check] [--template] [FILE]...` | format in place; `--check` writes nothing and exits 1 on a diff |
| `tot check [--strict] [--template] [--schema=F] [FILE]...` | parse and report diagnostics |
| `tot build [--check] [--out=F] [--set=N=V]... FILE` | build a `.tott` template into a document |
| `tot merge [FILE]...` | fold documents together, left to right |
| `tot get [--raw] <PATH> [FILE]` | print the one value at PATH |
| `tot set <PATH> <VALUE> [FILE]` | write VALUE at PATH and print the document |
| `tot to <json\|yaml\|toml> [FILE]` | write this document as another format |
| `tot from <json\|yaml\|toml> [FILE]` | read another format and write tot |

No FILE means stdin, and so does a FILE of `-` (a file actually named `-` is `./-`). For `fmt`
and `check`, every input is processed before exiting — one bad file doesn't hide the rest.
Exit codes: `0` ok, `1` the input didn't answer the request (unformatted, unparseable, or no
such path), `2` I/O or bad arguments.

Flags: `--raw` (`get`, `set`), `--create` (`set`), `--null=set|delete` (`merge`, default
`set`), `--compact` (`to json`),
`--null=omit|error` (`to toml`, default `omit`). A flag that doesn't apply to the chosen format
is an error, not a no-op. A bare `--` ends the flags — `-` is a bareword character, so `--foo`
is a legal key and a legal path, and a file can be named that way: `tot get -- --foo config.tot`.

`check --strict` adds one lint: **a member's value must begin on its key's line.** A `{`, `[`,
or `"""` may still run past it — only the start has to sit beside the key. Everything it
reports is legal tot, so it's opt-in. It exists because there's no separator between members:
a missing value shifts every member after it, and while quoted string values catch nearly all
of those at the offending token, keeping members on one line is what makes the error land in
the right place every time. **`tot fmt` fixes what it reports** — the formatter pulls the
value back up onto the key's line. `--strict` and `--schema=` are separate questions and
compose; a document can fail both at once.

`check --schema=` validates a document's shape. **A schema is a tot document that looks like
the documents it describes**, with a type where each value would be:

```tot
name    "string"
listen  {host "string" port "int" tls? "bool"}
regions ["string"]
labels  {* "string"}
retries "int|null"
```

Types are `any`, `string`, `int`, `float`, `bool`, `null`, joined with `|`. **`?` on a key**
makes the member optional — it goes on the key because presence is a property of the member,
which also means it works for a `{…}` or `[…]` value. **`*` as a key** covers every other key;
without one an undeclared member is an error, since catching a typo is most of the point.

Type names are quoted because a schema is tot, and a bare word is never a value in tot. That's
also what makes a schema line up with the config beside it. [examples/config.schema.tot](examples/config.schema.tot)
describes [examples/config.tot](examples/config.tot).

Every violation is reported, not just the first, and each one names a `tot get` path and points
a caret at the key. A typo gives you two — the name that's missing and the name that isn't
known — because either alone sends you to the wrong place. No enumerations yet; the spec says
why.

### Templates

A `.tott` file is tot plus one thing: a `(head arg…)` **form**, wherever a value goes,
evaluated at build time and replaced by its value. `tot build` turns one into a `.tot`
document, and the document is what you commit and everything else reads.

```tott
name      "example-service"
replicas  (if (param "prod") 5 1)
image     (str "registry/" (param "name") ":" (param "tag"))
regions   (import "regions.tot")
endpoints (map (str "https://" (it) ".example.net") (import "regions.tot"))
retries   (get "limits.retries" (import "defaults.tot") 3)
```

```bash
tot build --set=prod=true --set-raw=tag=v1.4.2 --out=config.tot config.tott
```

**The data language doesn't change.** Parens are ordinary bareword characters in `.tot` and
delimiters only in `.tott`, so `(a) 1` is still the key `(a)` and `@type` / `$ref` are still
bare — which is exactly what reserving `@` or `$` would have cost, and those are JSON-LD and
JSON Schema, the documents `from json` exists to read. Parens won because **they never appear
in data**: what's inside them is computed, what's outside isn't, and you can see which without
knowing the form set.

There are seven forms and **no way to define an eighth**:

| | |
|---|---|
| `(param "name")` | a build parameter, or a failure if it wasn't set |
| `(param "name" default)` | …or `default`, when it wasn't set |
| `(if cond then else)` | `cond` must be a boolean; only the branch taken is evaluated |
| `(str a b …)` | joins strings, numbers, and booleans |
| `(import "file")` | that file's value, resolved relative to the importing file |
| `(get path value)` | the value at `path` inside `value` |
| `(get path value default)` | …or `default`, when there's nothing there |
| `(map body list)` | `body` evaluated once per element of `list` |
| `(it)` | the element the enclosing `(map …)` is on |

The fixed set is the whole discipline — the moment a template can define a function, people
write libraries and a config file becomes a program you have to read as one.

Things worth knowing:

- **A form goes where a value goes, and only there.** A computed key would make a document's
  shape depend on evaluating it. Splicing one document's members into another is `merge`;
  embedding one value is `import`.
- **A `param` name and an `import` path are written down, not computed**, so you can see what a
  template needs and what it pulls in without running it.
- **`if` takes a boolean** — no truthiness here either. The branch not taken isn't evaluated,
  so it can import a file this configuration doesn't have.
- **`get` reads out of a value you hand it**, never out of the document being built — that would
  make a template's meaning depend on the order its members happened to be evaluated in, and let
  a document refer to itself. Its path *may* be computed, unlike a `param` name or an `import`
  path: those are static because they're the build's inputs, and a path isn't one. Selecting by
  parameter, `(get (param "env") (import "environments.tot"))`, is the ordinary reason to want
  one. The third argument is the only way to reach a member that may be missing, since `if`
  wants a boolean and there's no `has` — but it covers a member that isn't there and an index
  past the end, and nothing else. A step into the wrong *kind* of value is the document being
  shaped differently than the template thinks, and stays a failure with or without a default.
- **`map` needs a placeholder, and `(it)` is a form because everything computed in tot sits
  inside parens.** `_` would have been shorter and would have broken the sentence the paren
  sigil rests on. A `map` may not appear inside another `map`'s *body*, so `(it)` names the
  element of exactly one of them and never needs a shadowing rule; in the *list* argument it's
  fine, since that one finishes before the body runs. An import is a wall for the same reason —
  an imported file is parsed on its own, so `(it)` in one is an error rather than a reach into
  whatever imported it, which is also what keeps the build cache sound. Both are parse errors,
  so `tot check` catches them without building anything.
- **Parameters come from the command line and nowhere else**, so a build reproduces anywhere.
  `--set=N=V` takes a tot value, `--set-raw=N=V` takes a literal string — the same split as
  `set` / `set --raw`. Want the environment? `--set-raw=env="$ENV"` puts it in plain sight.
- **`--check` is the one that earns its keep**: it builds and compares against the document on
  disk (`config.tott` → `config.tot`), so CI catches a committed document that's drifted from
  its template, the way `fmt --check` catches formatting.
- Imports must be **acyclic**, and a failure names the file it happened in plus the chain that
  reached it. **Each file is built once** however many times it's imported — its value doesn't
  depend on who imported it, so a build stays linear in the size of the graph instead of
  exponential in its depth. Sharing a fragment is the ordinary reason to have one.
- `build` refuses a `.tot` file: a document is already built, and reading one as a template
  would report its parens as forms. Extensions are read case-insensitively, since on Windows
  `CONFIG.TOT` and `config.tot` are one file. `--out` won't overwrite the template it's
  building from **or any fragment it imports** — both are files being read, and there's nothing
  to recover either from. A parameter set twice is refused rather than resolved, the way a
  duplicate key is.
- **`tot check` reads templates**, and reading one checks its forms — unknown head, wrong
  argument count, computed `param` name or `import` path. `--strict` applies its one lint too;
  the parity hazard is the language's, and a form is one more value that can land on the wrong
  line. **`--schema` against a template is refused**: a schema says what shape a *document* has,
  and `(param "x")` could be anything until it's built. Build first —
  `tot build c.tott | tot check --schema=shape.tot` — which is what the error tells you.
- **`tot fmt` formats templates**, so a `.tott` file is kept honest like everything else here.
  A form is one more bracketed shape with the same rule as a collection — inline vs. block is
  yours, spacing is normalized, nothing is reflowed. The head stays with the opening (`(str`),
  and a block form closes with `)` on its own line, the way `}` and `]` do.

  The extension picks the dialect. Stdin has none, so it's a document unless you pass
  `--template` — sniffing the contents would be exactly the implicit typing tot exists to
  avoid. **One consequence worth knowing:** `"(a)"` is unquoted to `(a)` in a `.tot` file and
  stays quoted in a `.tott` one, because unquoting it there would turn a key into a form.

[examples/service.tott](examples/service.tott) builds
[examples/service.tot](examples/service.tot), and a test keeps them in step.

`merge` is base-plus-overlays:

```bash
tot merge base.tot staging.tot regional/eu.tot
```

**Two objects merge member by member; anything else is replaced whole.** An array replaces
rather than appending — concatenation can't be undone by a later layer, so an overlay that
could only ever add would be a one-way door. A change of kind replaces too, at the root or
anywhere else. Base members keep their position; new ones append.

An overlay's `null` sets the member to null, because null is a real value here. `--null=delete`
removes it instead, which is how a layer takes something away — nested, so `a {b null}` deletes
`a.b`. A `null` at the root or inside an array is data either way.

One bad layer ends the run (there's a single output, and a partial merge is worse than none),
and comments don't survive, the same way they don't through `from`.

`get` takes a path where `.` selects a member and `[n]` an element:

```bash
tot get listen.port config.tot
```

**A `.` nests in a path but not in a document**, where it's an ordinary bareword character — so
a key holding one gets quoted: `tot get '"com.example".level'`. That's the only place the two
spellings of a key differ. Output is tot, so it pipes back into `tot to json` and friends;
`--raw` prints a string with no quotes or escapes, for `PORT=$(tot get --raw ...)`. A path the
document doesn't have is exit 1 and names what was there instead. No wildcards or filters — if
you want those, `tot to json | jq`.

`set` is the dual, and takes a value spelled the way `get` prints one:

```bash
tot set listen.port 8080 config.tot
tot set --raw name svc config.tot        # a string, without typing '"svc"'
```

So a string needs its quotes (`tot set name '"svc"'`) unless you use `--raw`, and a brace-less
`host "::" port 80` is an object here exactly as it is at the top of a file.

**The last step of the path may be new** — adding a member is what setting is for — but the
steps before it must exist, unless `--create` builds them. That's the default because a
mistyped path is likelier than a genuinely missing branch, and a silent success hides the typo.
`--create` never replaces something already there, and never invents array elements.

`merge`, `get`, and `set` all read and write documents, so they chain: `tot merge base.tot
prod.tot | tot set replicas 3 | tot to json`. None of them keep comments.

`from json` does no conversion. JSON is already tot, so it just reparses and reformats.

`from` writes a string as a `"""` block when it has line breaks and every line reads back
unchanged, so converted shell snippets and banners stay readable instead of collapsing into
one `\n`-laden line. A line ending in a space or tab falls back to a quoted literal.

The formatter **preserves inline vs. block** and never reflows — running `fmt` over minified
JSON gives you back one correctly-punctuated long line, not an exploded document. It unquotes
keys where legal, indents two spaces, and emits LF.

**LF is part of the canonical form, so `fmt` rewrites a CRLF file.** On Windows that matters:
with git's default `core.autocrlf=true`, `fmt` writes LF, git converts it back to CRLF on the
next checkout, and `fmt --check` reports the file unformatted forever — CI can never go green.
Check `.tot` files out with LF, as [.gitattributes](.gitattributes) in this repo does:

```
*.tot text eol=lf
```

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
let text  = tot::format_template(src)?;           // the same, for a .tott source
let text  = tot::format_value(&value);            // Value -> tot
let folded = tot::merge(documents, tot::Nulls::Set);   // or merge_into(&mut base, overlay, …)
let json  = tot::json::to_string_pretty(&value);
let warns = tot::lint(src)?;                      // -> Vec<Warning>, all legal-but-risky
let warns = tot::lint_template(src)?;             // the same, for a .tott source
let bad   = tot::Schema::parse(shape)?.check(src)?;    // -> Vec<Violation>
let at    = tot::Path::parse("a.b[0]")?.get(&value)?;
let v     = tot::parse_value(arg)?;               // text in a value position, not a file
let doc   = tot::Template::parse(tott)?.evaluate(&params)?;   // or .build(&params, &mut imports)
```

`Template` keeps its own source and name, so a failure three imports deep still draws its caret
in the right file. The library does no I/O — `(import …)` goes through an `Imports` trait, so a
build can be driven from the filesystem, an archive, or a map in a test without any of them
being special.

`parse_value` uses the same grammar as `parse`; the difference is the diagnostic. A lone
bareword in a file is most likely a key that lost its value and is reported that way, but as a
value there's no key to lose one, so it's reported as a string needing quotes.

`Path::parse` and `Path::get` are separate calls because their failures are different problems
— a malformed path is your bug, a missing one is the document's answer. Spans on a path error
index into the *path*, not the document.

There is no JSON *parser* and there doesn't need to be — `parse` reads JSON directly.

Errors carry a span: `e.render(src)` gives a caret diagnostic.

To edit a parsed document, `Value::get_mut` chains and `Map` has `get_mut` / `remove`:

```rust
let mut config = tot::parse(src)?;
*config.get_mut("listen").unwrap().get_mut("port").unwrap() = Value::Integer(Integer::from_i64(9090));
config.as_object_mut().unwrap().remove("stale");
```

`Map::insert` **refuses** a key that's already there and returns `false` — duplicates are a
parse error in the language, and last-wins isn't a rule tot has. Use `get_mut` to replace.

Numbers keep their original lexeme, so integers wider than `i64` survive a round trip.
`Integer` equality is value equality, but **`Float` equality is lexical** — `1.0 != 1.00`.
Compare `as_f64()` if you mean value equality.

The `tot` crate has **no dependencies by default**. YAML and TOML need third-party parsers, so
they live in `tot-cli` instead.

### serde

Behind the `serde` feature, off by default:

```toml
tot = { version = "0.1", features = ["serde"] }
```

```rust
let config: Config = tot::from_str(src)?;         // any DeserializeOwned
let text          = tot::to_string(&config)?;     // any Serialize
let value         = tot::to_value(&config)?;      // -> Value
let config: Config = tot::from_value(&value)?;    // borrows, so `&'a str` fields work
```

Both directions go through `Value` rather than straight to text — the formatter and parser
already do that work well, and a streaming implementation would be a second copy of both.

- **A value round-trips, not a document.** Comments, blank lines, and inline-vs-block choices
  live in the CST, which serde never sees. `to_string` writes block form like any converter.
- Errors name the offending value, spelled as a path: ``invalid type: string "8080", expected
  u16 at `listen.port` ``. That string is a real path — hand it to `tot get`. `Error::path()`
  gets it alone; `Error::parse_error()` gets the span when the document didn't parse at all.
- Enums are externally tagged: `"off"` for a unit variant, `retry 5` for anything else.
- `null` is `None`; `false` and `0` are `Some`.
- `1.0` will not deserialize into a `u32` — the language keeps integers and floats apart, and
  so does this. An integer *will* deserialize into an `f64`.
- Integer map keys (`BTreeMap<u16, _>`) work in both directions; float keys work in neither.
- Infinity and NaN are a serialization error rather than a silent `null`.
- `Value` itself is `Serialize + Deserialize`, so part of a document can stay untyped.
- **Float lexemes are normalized** — serde carries the number, not the spelling, so `1.` comes
  back as `1.0`. Since `Float` equality is lexical, a `Value` with such a lexeme is `!=` itself
  after a serde round trip. Integer lexemes are unaffected, up to 128 bits.

## Layout

```
src/                tot — library, zero dependencies
  lex.rs            tokenizer
  parse.rs          recursive descent -> Value
  cst.rs            lossless tree: comments, blank lines, inline/block choice
  fmt.rs            formatter (over the CST) + Value -> tot emitter
  lint.rs           opt-in checks (over the CST); nothing here is a language rule
  merge.rs          folding documents together, left to right
  template.rs       `.tott`: the data grammar plus forms, and the evaluator
  path.rs           `a.b[0]` paths — a CLI convenience, not part of the language
  schema.rs         checking a document's shape against a schema written in tot
  json.rs           JSON output only
  serde/            optional; ser.rs and de.rs, both via Value
  value.rs          Value, Integer, Float, Map
  error.rs          Span, Error, shared caret rendering
cli/                tot-cli — binary `tot`; deps: toml, yaml_serde
  build.rs          resolving `(import …)` against the filesystem
```

`parse` and `format` are separate walks over the same token stream. `format` validates with
`parse` first, then rebuilds a CST that keeps the trivia `parse` throws away — trivia is read
straight out of the gaps between token spans, so there's no second lexer.

## Build

```bash
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

**`--all-features` matters** — `serde` is off by default, so without it the serde tests compile
to nothing and pass silently. Run clippy both ways; the feature gate is easy to get wrong.

Edition 2024, Rust 1.88. Formatter tests assert two properties on every fixture, not just
expected output: formatting preserves the parsed value, and formatting is idempotent.

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs all of the above on Linux,
Windows, and macOS, plus three things a local run tends to skip: the MSRV the manifests claim,
the tool's own guarantees over `examples/` (`fmt --check`, `check --strict`, `build --check`,
and the schema), and a publish dry run. That last one is not ceremony — a missing license and a
path dependency without a version are both invisible until you try to publish.

## Next

`tot merge`, `tot set`, schema validation, the `.tott` template layer with all seven of its
forms, and `fmt` and `check` for templates are all built. Still open, in rough order:

- **Enumerations in schemas.** The most obvious missing check, with no good spelling yet —
  `["debug" "info"]` already means an array of one element type, so it can't also mean a choice
  between two literals. [The spec](SPEC.md#ranked-ahead-of-all-of-it) has the details.
- **In-place editing that keeps comments.** `set` and `merge` fold values, so they can't write
  back over a hand-written config without losing its comments. Doing that properly means
  splicing text through the CST, which is a different and larger feature.
- **A lint of its own for templates.** A template gets the one lint the language has, since the
  parity hazard is the language's. Whether there's a second rule worth having — about where a
  form's arguments sit, say — is unmeasured.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. This is the Rust ecosystem's convention: MIT for anyone who finds Apache-2.0's
notice requirements heavy, Apache-2.0 for anyone whose legal team wants the explicit patent
grant.

Unless you state otherwise, any contribution you intentionally submit for inclusion in this
work shall be dual-licensed as above, with no additional terms or conditions.