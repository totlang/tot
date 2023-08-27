# Tot

A configuration language for sane programmers.

The full language specification is located in [SPEC.md](SPEC.md).

## Features

* Whitespace-based format that _does not_ require indentation
* Simple, limited syntax
* JSON-style objects and lists
* Reference values
* File import
* Non-Turing complete Lisp-style expressions
* Compatible with:
    * JSON
    * YAML
    * TOML

## Implementations

* Rust: [tot-rs](https://github.com/totlang/tot-rs)
* Deno: [tot-deno](https://github.com/totlang/tot-deno)

## Example

```clojure
// C-style comments
/*
and block comments
*/

// Key-value pairs are based off of newlines
title "Tot Example"

// Strings can be split across multiple lines
description "
A cool
\"multiline\" description
"

/*
Keys can have no value denoted by an empty expression.
*/
unit-value null

// Keys are always parsed as Strings
// The key names cannot contain double quotes unless they are escaped (i.e. \")
1 2
tim's-value "value"

my-int 1
my-float 1.0

// JSON-style objects (called dicts in Tot)
counter {
    count 10
    should-restart false
    inner-config {
        timezone "GMT"
    }
}

// JSON-style lists
things-i-like [
    "turtles", // Note that commas are optional in lists
    22
    true
    false
]

// Reference values
my-title (& title) // Equal to my-title "Tot Example"

// Reference value access
my-timezone (& counter inner-config timezone) // Equal to my-timezone "GMT"

// Expressions
// A subset of Lisp syntax that is intentionally not Turing-complete
calculated-number (+ 1 2) // Equal to calculated-number 3

// Generators
(gen person [name description] {
    name name
    description description
    favorite-food "unknown"
})
some-person (person "Tim" "A cool person")

(gen calculate-square [n] (* n n))
square-of-2 (calculate-square 2)

```

## Tests

The test cases in the `test-cases` directory should be runnable for every implementation.

## License

MPL-2.0
