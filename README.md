# Tot

A configuration language for sane programmers.

Currently the only documented specification is located in [`tour.tot`](test-cases/tour.tot) ([SPEC.md](SPEC.md) is under construction).

## Features

* Newline-based format
* JSON-style objects and lists
* Reference values
* File import
* Non-Turing complete expressions

## Implementations

* Rust: [tot-rs](https://github.com/totlang/tot-rs)

## Example

```tot
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
Keys with no matching pair are parsed as unit values
There is no ambiguity since there can only be
1 key-value pair per line
*/
unit-value

// Keys are always parsed as Strings
// The key names cannot contain quotes (', ")
// and they cannot start with a colon (:) since colons are reserved
// for Tot-specific syntax
1 2
tims-value "value" // Cannot use a apostraphe for the key name

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
my-title &title // Equal to my-title "Tot Example"

// Reference value access
my-timezone &counter("inner-config")("timezone") // Equal to my-timezone "GMT"

// Expressions
// A subset of Lisp syntax that is intentionally not Turing-complete
calculated-number (put (+ 1 2)) // Equal to calculated-number 3

calculated-list [
    (for i 2 (put {
        name "counted object"
        count i
        counter &counter
    }))
]
/*
Equal to:
calculated-list [
    {
        name "counted object"
        count 0
        counter &counter
    }
    {
        name "counted object"
        count 1
        counter &counter
    }
]
*/

// Generators
:gen person(name, description="pretty cool") {
    name name
    description description
    favorite-food "unknown"
}
some-person &person("Tim")

:gen prefilled-list(n) [
    "some string first"
    (for val n (put val))
]
count-up &prefilled-list(3)

// Imports
:use other_file.tot
my-imported-value &other_file.tot("my-value")

:use to_be_renamed.tot renamed-import
my-renamed-import-value &renamed-import("cool-value")("nested")

```

## Tests

The test cases in the `test-cases` directory should be runnable for every implementation.

## License

MPL-2.0
