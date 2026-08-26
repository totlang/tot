# tot — specification (draft v0.4)

`tot` is JSON with the punctuation removed. Same data model, same semantics, fewer
characters to type and fewer to get wrong. Whitespace is *only* a delimiter — it never
carries structure the way it does in YAML.

```tot
my-name "tim"

address {
  street "100 main st"
  zip 123456
  country "united states"
}

"favorite food" [
  "tacos"
  {
    name "fries"
    kind "curly"
    rating 10
  }
]
```

## Design goals

1. **The JSON data model**, with JSON's single number type split into integers and floats.
   object, array, string, integer, float, `true`, `false`, `null`. No dates, no anchors, no
   tags, no includes, no interpolation. (Composing documents is a real need and is taken up
   under [Composition](#composition-prospective) — the goal there is to serve it *without*
   putting any of this into the data language.)
2. **Every JSON document is a valid tot document.** Not "convertible to" — literally valid,
   byte for byte. Paste JSON into a `.tot` file and it parses.
3. **No significant whitespace.** A tot document may be written entirely on one line, and
   reformatting can never change what it means.
4. **No implicit typing.** `no` is not `false`, `2024-01-01` is not a date, `NO` is not Norway.
   A value is a string only if it is quoted.
5. **One obvious way to write things**, enforced by `tot fmt`.

## Lexical structure

### Whitespace and skipped characters

The following are *skipped* between tokens and are all equivalent to each other:

| | |
|---|---|
| space `U+0020`, tab `U+0009`, LF `U+000A`, CR `U+000D` | JSON's whitespace set |
| `,` and `:` | for JSON compatibility |
| `# …` to end of line | comment |

Treating `,` and `:` as whitespace is what makes goal #2 fall out for free: `{"a": 1, "b": 2}`
tokenizes identically to `{a 1 b 2}`. It also makes trailing commas legal. `tot fmt` deletes
them, so canonical tot never contains either.

There is no block comment form; `#` always runs to the end of the line. `#` was chosen over
C-style `//` so that `/` stays legal in barewords — under `//`, `foo// bar` silently lexes as
the bareword `foo//` rather than a key and a comment, and path-shaped keys like
`path/to/thing` would need quoting.

Any other Unicode whitespace outside a string (non-breaking space, ideographic space, …) is a
**lexical error**, not a delimiter. Silently absorbing `U+00A0` into a bareword is a footgun
worth spending an error message on.

### Tokens

```
token    = STRING | BAREWORD | "{" | "}" | "[" | "]"
BAREWORD = 1*( any Unicode scalar except whitespace and , : " { } [ ] # = )
STRING   = single-line-string | multi-line-string
```

Tokens are self-delimiting: `a"b"` is two tokens and `a{b 1}` is three. `tot fmt` inserts the
spaces you'd expect; the parser does not require them.

### Reserved characters

`#` and `=` are excluded from barewords. `#` because it introduces comments; `=` purely so the
lexer can produce a precise diagnostic for the habit people bring from TOML and INI. Keys
containing either must be quoted.

| | |
|---|---|
| `=` | tot has no assignment operator — write `key value`, not `key = value` |

### Strings

Single-line strings are exactly JSON: double quotes, escapes `\" \\ \/ \b \f \n \r \t \uXXXX`,
surrogate pairs for astral characters, literal control characters forbidden. Input must be
valid UTF-8.

Multi-line strings are delimited by `"""`:

```tot
motd """
  hello
    indented
  world
  """
```

produces `"hello\n  indented\nworld"`.

1. Only whitespace may follow the opening `"""` on its line, and it is discarded. Content
   begins on the next line.
2. The closing `"""` **owns its whole line**: only horizontal whitespace may come before it,
   and only horizontal whitespace may come after it. The whitespace before is the
   **indentation prefix**.

   Anchoring both ends is one rule instead of half of one, and it is the same shape the
   emitter already assumes when it escapes a `"""` only where one opens a line. The cost is
   that `a """…""" b 1` and `{a """…"""}` are no longer legal — a token may not follow the
   delimiter on its line. That buys the diagnostic for the mistake people actually make: an
   unescaped `"""` in the content would otherwise close the string early and report an error
   somewhere further down that has nothing to do with it. The formatter never writes anything
   after a closing delimiter, so no canonical document is affected.
3. Every content line must begin with that prefix, or be entirely whitespace. The prefix is
   stripped from each line; whitespace-only lines become empty. A non-blank line that doesn't
   start with the prefix is an error. The prefix must match byte-for-byte, so mixing tabs and
   spaces is caught rather than guessed at.
4. The newline before the closing `"""` is a delimiter, not content — the value has no trailing
   newline unless you write one with `\n`.
5. Line endings inside the string are normalized to `\n` regardless of the file's line endings,
   so a document does not change meaning when it crosses a CRLF boundary.
6. The same escapes apply, plus `\` at end of line as a line continuation, which drops the
   backslash and the newline. A lone `"` or `""` is literal; write `\"""` for three.

Anchoring the indentation to the closing delimiter rather than to column zero is what keeps
goal #3 true: `tot fmt` can re-indent a block containing a multi-line string, rewriting the
prefix on every line, and the value is unchanged.

Raw (escape-free) strings are deliberately absent — see open questions.

### Numbers

A number is a **float** if it contains a `.` or an exponent, and an **integer** otherwise.

```
number = "-"? ( digits ("." [0-9]*)? | "." [0-9]+ ) ([eE] [+-]? [0-9]+)?
digits = "0" | [1-9][0-9]*
```

```tot
answer   42          # integer
offset   -7          # integer
ratio    1.5         # float
whole    1.          # float, 1.0
tiny     .1          # float, 0.1
avogadro 6.02214e23  # float
```

This extends the JSON grammar in exactly two places: `1.` and `.1`. Everything JSON forbids,
tot forbids too — no leading zeros, no leading `+`, no hex, no underscores, no `inf`/`nan`.
Goal #2 holds in the direction that matters: every JSON number is a tot number. In the other
direction the two dot forms are the only ones needing normalization on export, to `1.0` and
`0.1`.

**An exponent makes a float even with no `.` present.** `1e-5` plainly cannot be an integer,
and special-casing the exponent's sign is a worse rule than treating every exponent the same
way; `6e23` is a float here for the same reason it is in TOML and in Rust.

Numbers keep their lexeme, so `1E-3` stays `1E-3` and an integer wider than `i64` — a `u64`
snowflake id that arrived in a JSON file — is not clamped or quietly turned into a float.

**An integer therefore has no range limit, but a float must denote a real `f64`.** `1e999` is
a parse error, not an infinity: tot has no way to *write* an infinity, so a literal that means
one has no value in the language. Accepting it would make a document that parses and formats
but that no converter can write — and the failure would surface far from the literal, naming
`inf` for a document that never says it. Underflow is not the same thing: `1e-999` is zero,
which is a value tot has, and its lexeme survives like any other.

> Note on the example above: `zip 123456` is an *integer*. Zip codes with a leading zero
> (`01234`) are not valid tot numbers and must be written `"01234"` — which is what you
> wanted anyway.

## Grammar

```
document = members EOF
         | value EOF                      ; only when the whole input is one value

members  = ( key value )*
key      = STRING | BAREWORD
value    = STRING | INTEGER | FLOAT | "true" | "false" | "null" | object | array
object   = "{" members "}"
array    = "[" value* "]"
```

**The top level is an object body with the braces left off.** That covers ~all real config
files. A document whose entire content is a single value is that value instead, so
JSON documents with a scalar or array root still parse. This is unambiguous: `{`, `[`, and
number/boolean/`null` literals can never begin a `members` production, and a lone `"foo"`
fails as `members` (no value follows) before succeeding as `value`.

An empty document is the empty object.

### Keys are barewords, values are quoted

This asymmetry is the whole trick, and it is load-bearing.

A key is *always* a string, so a bareword in key position is unambiguous — quote it only when
it contains a delimiter or reserved character (`"favorite food"`, `"a=b"`). `123 "x"` is the
key `"123"`. Dots, dashes, and slashes are ordinary bareword characters: `path/to/thing`,
`com.example.setting`, and `my-name` are all bare.

A value's type is determined by its syntax, so a **bareword in value position must be a
number, `true`, `false`, or `null`.** A bare `curly` is a parse error:

```
kind curly
     ^^^^^ expected a value; string values must be quoted: `"curly"`
```

The alternative — bare strings as values — was rejected because two adjacent barewords are
undecidable. In `country united states` there is no rule that recovers the author's intent,
and `a b c d` would silently mean `{a: "b", c: "d"}`. That is the YAML failure mode this
language exists to avoid. The cost is a pair of quotes; the benefit is that no tot document
can mean something other than what it looks like.

### Duplicate keys

A parse error. JSON permits them and every implementation resolves them differently; tot
picks the one behaviour that can't surprise anyone.

### The parity hazard

Because members have no separator, a member with a missing value shifts everything after it:

```tot
debug            # author meant `debug true`
port 8080
```

parses as `{debug: "port"}` … then `8080` is a key with nothing after it. Three mitigations,
in order of importance:

1. Requiring quotes on string values catches most of these at the exact token (above, `port`
   is a bareword in value position → error points at line 1, not at EOF).
2. Errors are reported as *"key `debug` has no value"* by tracking the key's span, never as
   *"unexpected EOF"*.
3. `tot check --strict` warns when a member's value does not begin on its key's line. A `{`,
   `[`, or `"""` may still run past it — only the *start* of the value has to sit beside the
   key. Whitespace stays non-structural in the language; the tooling picks the convention that
   makes the error land in the right place every time.

## Interop

| direction | fidelity | notes |
|---|---|---|
| JSON → tot | lossless, total | no-op read; duplicate keys become an error |
| tot → JSON | lossless, total | comments dropped; `1.` and `.1` normalized |
| YAML → tot | lossy | anchors/aliases resolved & inlined; tags, non-string keys, dates, multi-document streams unsupported |
| tot → YAML | lossless, total | emitted with explicit quoting |
| TOML → tot | lossy | datetimes become strings (reported on stderr); integers and floats map 1:1 |
| tot → TOML | lossy | root must be an object; `null` omitted (below); sub-tables hoisted below plain values, since TOML's syntax requires it |

"Mostly compatible" with YAML/TOML in the honest direction: tot's model is the intersection,
so anything that survives a round trip through tot is portable to all three.

Splitting numbers costs nothing on the JSON side — JSON draws no such distinction, so an
integer emits as `1` and a float as `1.0`, and reading that back recovers the same types. It
is what makes TOML, which does distinguish them, map exactly rather than by guesswork.

### Nulls on TOML export

TOML has no null. Every `null` is **omitted**: an object member with a null value is dropped
entirely, and a null array element is removed (shortening the array). Each omission is reported
on stderr with its path, and `--null=error` makes them fatal instead. An object left empty by
this becomes an empty table.

This is lossy by construction — `tot → toml → tot` does not round-trip through a null.

## Canonical form (`tot fmt`)

- Two-space indent, LF endings, trailing newline.
- `,` and `:` deleted.
- Keys eagerly unquoted wherever legal: `"address"` → `address`. Pasted JSON is rewritten
  into tot on first format; that's the point.
- **Inline vs. block is the author's choice, preserved.** A collection whose source contains a
  newline between its brackets is formatted one member/element per line, `{` or `[` on the
  key's line and the closing bracket on its own line at the key's indent. A collection with no
  newline in it stays on one line, normalized to single spaces: `[1  2]` → `[1 2]`.
  The formatter never reflows or re-wraps; there is no line-length rule.
  - A consequence worth knowing: running `tot fmt` over a minified JSON file gives you back
    one long line, correctly punctuated. Reformatting cannot invent structure the author
    didn't write. Exploding it is a converter's job, not the formatter's.
  - The brace-less root has no brackets to hold an inline form, so its members are always one
    per line. Write `{a 1 b 2}` if you want the whole document on one line.
  - An empty collection collapses to `{}` / `[]` even if it was written open, unless it
    contains a comment.
  - Inline collections carry no inner padding: `{a 1}` and `[1 2]`, matching JSON.
- Converters (`tot from json`, etc.) have no author intent to preserve and emit block form for
  everything except empty collections, which stay `{}` / `[]`.
  - A string is written as a `"""` block when it contains a line break and every line reads
    back unchanged. A line ending in a space or tab rules the whole string out — the reader
    blanks a whitespace-only line, and trailing whitespace is the first thing an editor
    strips. Everything else is handled by escaping: a backslash, a carriage return that would
    otherwise be eaten as a line ending, and a `"""` opening a line, which would close the
    string. Quotes anywhere else are left bare so an embedded script stays readable.
- Multi-line strings are re-indented one level inside their member; the value is unchanged.
  This is well defined only because the indentation prefix is anchored to the closing
  delimiter, which moves along with the content.
- Comments attach to the member that follows them, or stay trailing if they followed a value
  on the same line. A comment between a key and its value has no natural home and moves above
  the member.
- At most one consecutive blank line, preserved. Blank lines are dropped at the start and end
  of a block and of the document.

## CLI

```
tot fmt [--check] [FILE]...       format in place, or stdin to stdout
tot check [--strict] [FILE]...    parse and report errors
tot merge [--null=…] [FILE]...    fold documents together, left to right
tot get [--raw] <PATH> [FILE]     print the one value at PATH
tot to <json|yaml|toml> [FILE]    write this document as another format
tot from <json|yaml|toml> [FILE]  read another format and write tot
```

- Extension `.tot`. With no FILE, input is read from stdin. A FILE of `-` is stdin as well, so
  a pipeline can be one layer of a merge; a file actually named `-` is `./-`.
- `--check` on `fmt`: write nothing, exit 1 if any file would change.
- `--strict` on `check`: also warn about the split-member shape above. Everything it reports
  is legal tot, so it is opt-in.
- `--raw` on `get`: print a string with no quotes and no escapes. Other values are unaffected,
  being unquoted already.
- `--compact` on `to json`: one line instead of indented.
- `--null=omit|error` on `to toml`, defaulting to `omit`.
- Exit codes: `0` success, `1` the input did not answer the request — a file is unformatted, a
  document failed to parse, or a path was not found — `2` a file could not be read or written
  or the command line was wrong. Every input is processed before exiting, so one unparseable
  file in a directory does not hide the rest.
- A flag that cannot apply to the chosen format is an error rather than a silent no-op:
  `tot to yaml --compact` is refused.
- A bare `--` ends the flags. `-` is an ordinary bareword character, so `--foo` is a legal key
  and a legal path, and a file may be named that way; without a terminator neither would be
  reachable.
- `tot fmt` pulls a value back onto its key's line, so it repairs exactly what
  `check --strict` reports.

### Merge

`merge` folds documents left to right. **Two objects merge member by member; anything else is
replaced whole.** See [Composition](#composition-prospective) for why the alternatives were
rejected.

- An **array replaces** rather than appending. Concatenation cannot be undone by a later
  layer, so an overlay that could only ever add would be a one-way door.
- A **change of kind replaces**, at the root as well as anywhere else. There is no sensible
  way to fold a string into an array, and guessing at one is how these systems become
  unpredictable.
- A member the base already has keeps its position; one only the overlay has is appended in
  the order the overlay wrote it. Key order is part of a document, so a merge does not
  reshuffle it.
- `--null=set` (the default) treats an overlay's `null` as the ordinary value it is.
  `--null=delete` removes the member instead, which is how an overlay takes something away.
  Deleting is a member-level operation: a `null` at the root, or inside an array, is data.
- Unlike `fmt`, one bad input ends the run. There is a single output, and a document merged
  from only some of its layers is worse than none.
- Comments do not survive, the same way they do not through `from` — the fold is over values.
- `from json` has no conversion step. Every JSON document is already valid tot, so it reads
  the input with the ordinary parser and reformats — the JSON direction needs no code at all,
  which is goal #2 paying for itself.

### Paths

A path is not tot syntax, and is not part of the language. It is the CLI's way of naming one
value, and it exists only because a shell needs one.

```
path    = step ( "." member | index )*
step    = member | index
member  = bare | STRING
bare    = ( any character legal in a bare key, except "." )+
index   = "[" [0-9]+ "]"
```

- `.` selects a member, `[n]` an element: `listen.port`, `routes[0].path`, `[2]` in a document
  whose root is an array.
- **A `.` nests here but not in a document**, where it is an ordinary bareword character. A key
  holding one is written quoted, with the ordinary string escapes: `"com.example".level`. This
  is the one place the two spellings of a key differ, and the reason a path is documented as
  its own little language rather than as a list of keys.
- A path the document does not have is exit 1, not 2: the command line was well formed, and
  the document simply did not answer it. A path that is not a path is exit 2.
- A miss names what was there, spelled the way a path spells it — a suggestion the reader
  cannot type is worse than none.
- Output is a tot document, so `tot get` composes with the rest of the CLI. A collection comes
  back whole; `--raw` is for the case where a shell wants the contents of a string and not a
  tot rendering of it.
- No wildcards, slices, or filters. That is a query language, and the reason to reach for one
  is a sign that the document should be converted to JSON and handed to `jq`.

### Implementation notes

- Two crates. `tot` is the library — parser, formatter, JSON output — and has **no
  dependencies**. `tot-cli` is the binary and carries the only two, a TOML parser and a YAML
  parser, since those are the formats tot cannot read on its own.
- Hand-written lexer + recursive-descent parser. The grammar is small enough that a generator
  buys nothing and costs error quality.
- A leading byte-order mark is skipped. It is not whitespace, so left alone it would become
  the first character of the first key.
- Keep spans on every token so diagnostics can point at the *key* whose value is missing.
- Integers and floats are separate types, each storing its original lexeme and parsing on
  demand. Integer lexemes are canonical (leading zeros are a parse error) so their equality is
  value equality; float equality is lexical, and `1.0`, `1.00`, and `1e0` are distinct values
  that compare equal only through `as_f64`.
- A `serde` `Serializer`/`Deserializer` pair so `tot` plugs into the Rust ecosystem, behind an
  off-by-default feature so the parser and formatter stay dependency-free. Both directions go
  through `Value` rather than to and from text: the formatter already writes a `Value` well and
  the parser already reads one, so streaming would be a second copy of both.
  - What round-trips is a *value*, not a document. Comments, blank lines, and the author's
    inline-versus-block choices live in the CST, which serde never sees — which is why the
    formatter has its own path and does not go through serde.
  - Enums are externally tagged: a bare string for a unit variant, `{Variant payload}` for the
    rest. That is the shape serde's own derive reads back.
  - A deserialization error names the offending value as a path, spelled the way the CLI spells
    one, so the location in a message can be handed straight to `tot get`.
  - The integer/float split is enforced in both directions: `1.0` will not deserialize into a
    `u32`, though `1` will deserialize into an `f64`. Infinity and NaN are an error rather than
    the `null` a JSON encoder writes.
  - A tot key is always a string, so an integer or boolean map key is written as its text and
    parsed back out of it. Float keys are refused in both directions.
  - Integer lexemes survive up to 128 bits, which is as wide as serde's model goes. Float
    lexemes do not: serde carries the number and not the spelling, so `1.` comes back as `1.0`
    and a `Float`, whose equality is lexical, does not compare equal to itself afterwards.
- Object key order is preserved in every direction.
- Recursion is depth-limited so that pathological nesting is a diagnostic, not a stack
  overflow.

## Composition (prospective)

The design deliberation for building larger documents out of smaller ones, kept here so the
reasoning survives the conversation that produced it. **`tot merge` is built** — its rules are
normative under [Merge](#merge) above. Everything below it in this section is neither
implemented nor decided.

The need is real: base configuration plus per-environment overlays, a fragment shared by
several documents, a value that appears in five places. Every config format grows this
eventually, and the ones that grew it badly — an unbounded expression language reachable from
any file — are now hard to read, which is the one thing tot exists to prevent.

The organizing principle below is that **the data language does not change.** A `.tot` file
still denotes a value you can see by reading it, and no consumer of tot has to become an
evaluator.

### Most of it needs no language at all

Layering is a CLI operation, not a syntax:

```
tot merge base.tot staging.tot regional/eu.tot
tot set <path> <value> [FILE]
```

`merge` folds documents left to right:

- **Objects merge recursively**, right wins on a conflicting scalar.
- **Arrays replace, they do not concatenate.** Concatenation cannot be undone by a later
  layer, and an overlay that can only ever add is the defect Kustomize needed strategic-merge
  patches to dig out of. Replacement is predictable; a layer that wants to append can say so
  with a path.
- **`null` sets null**, because null is a real value here. Deleting a member is a different
  operation, and a `--null=delete` flag would spell it — matching the `--null=omit|error`
  vocabulary `to toml` already uses.

`set` is the dual of `get`, and finishes that pair. It is not built yet.

**These come first, and not only because they are cheap.** They are the measurement: with
layering in hand, whatever composition is still awkward is a specific, nameable thing rather
than a guess. Designing an expression language before that is designing against an imagined
requirement.

`merge` landed at about seventy lines, most of which is the doc comment explaining why an
array replaces. `Map::get_mut` and `Map::remove`, added a commit earlier for unrelated
reasons, turned out to be exactly what a recursive fold needs.

### If syntax is still wanted: forms

A `(head arg…)` form, evaluated at build time and replaced by its value.

```tot
replicas (if prod 5 1)
image    (str registry "/" name ":" version)
regions  (import "regions.tot")
```

This is the least bad sigil, and the cost is measurable. `(`, `)`, `@`, and `$` are all
currently ordinary bareword characters:

```
(a) 1        -> {"(a)":1}
@type 2      -> {"@type":2}
$ref 3       -> {"$ref":3}
```

Any sigil comes out of that charset — the same trade `//` comments lost. But reserving `@` or
`$` would stop `tot fmt` unquoting `"@type"` and `"$ref"`, which are JSON-LD and JSON Schema,
precisely the documents `from json` exists to read. Parens in bare keys are far rarer, and
they carry the property the whole idea depends on: **parens never appear in data, so
computation is distinguishable from data by looking.**

For the same reason a `(str …)` form beats `"${name}"` interpolation. Interpolation makes
every string potentially computed and forces a reader to scan for it; a form keeps the
computation visibly outside the quotes.

### The constraint that has to be designed in first

**No user-defined functions.** A fixed set of builtins, no `defn`, no recursion, evaluation
total and terminating, no effects but `import` — which is resolved at build time and must be
acyclic.

This is the whole discipline. The moment `defn` exists, people write libraries, and a
configuration file becomes a program that has to be read as a program. Roughly six forms is
the target — `import`, `str`, `if`, `get`, `param`, `map` — and a seventh should be a
deliberate decision, not a convenience.

### Two file types

Forms live in a template file that builds to tot:

```
tot build config.tott -o config.tot
tot build --check config.tott
```

`--check` is the real prize: CI verifies the committed output still matches its source, the
way `fmt --check` verifies formatting. It also keeps every design goal above intact, because
the thing checked into the repository and read by every consumer is ordinary tot.

The split also divides the labor cleanly. **Splicing members into a document is `merge`;
embedding one value is `(import …)`.** Those are genuinely different operations, and a single
syntax made to do both is where these designs usually go muddy.

### Ranked ahead of all of it

**Validation is worth more than templating.** A document that builds successfully and is
wrong is the failure that actually gets hit, and generating documents makes it more likely,
not less. `tot check --schema schema.tot`, with the schema itself written in tot, needs no
language change at all and pays off immediately at the sizes that motivate composition in the
first place.

### Undecided within the above

- Whether `merge` needs `--at <path>` for merging a fragment somewhere other than the root, or
  whether `set` plus a shell pipeline covers it.
- Whether `param` reads from `--set name=value`, the environment, or both. The environment is
  convenient and makes a build unreproducible.
- Whether a template file is a distinct extension or a `.tot` file with a marker. An extension
  is honest; a marker is fewer file types to explain.
- Whether `import` resolves relative to the importing file or the invocation. Relative to the
  file is the only answer that makes a fragment relocatable.

## Open questions

1. **Raw strings.** Regexes and Windows paths mean `\\` everywhere. A no-escape variant
   (`'''…'''`, or `r"…"`) would fix it but adds a fourth string form.
2. **Trailing newline on `"""`.** Currently "the lines you see, joined by `\n`" — no trailing
   newline. Java text blocks do the opposite. Embedding a file's contents is the case that
   wants one; you'd write `\n` before the closing delimiter.
3. **Reserved `=`** exists only to produce a good diagnostic. Cheap to drop if the cost to the
   bareword charset isn't worth it.
4. **Float equality is lexical**, which is defensible for a format that preserves what you
   wrote but will surprise anyone who writes `a == b`. The alternative is storing floats as
   `f64` and giving up textual fidelity on export.
