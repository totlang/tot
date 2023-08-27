# Table of Contents

- [Data Type Overview](#data-type-overview)
- [Key](#key)
- [Value](#value)
- [Unit](#unit)
- [Boolean](#boolean)
- [Integer](#integer)
- [Float](#float)
- [String](#string)
- [List](#list)
- [Dictionary](#dictionary)
- [Expression](#expression)

# Data Type Overview

In general, data is structured as a `key` followed by a `value`.

| | Description | Example |
| --- | --- | --- |
| Key | Any value in the `key` position. | `my-key "my value"` |
| Value | Any text that is not a `key` or `comment`. | `"value"` |
| Unit | Denotes the absence of a value. | `my-key null` |
| Boolean | A `true` or `false` value. | `is-boolean true` |
| Integer | A whole number value. | `life 42` |
| Float | Any number with a decimal. | `pi 3.14` |
| String | Any text surrounded by double quotes. | `hello "world"` |
| List | A collection of values. | `some-list [1 "two"]` |
| Dictionary | A collection of key-value pairs. | `my-dict { hello "world" }` |
| Expression | An evaluatable expression. | `(+ 1 1)` |
| Line comment | A comment for a line. | `// comment` |
| Block comment | A multi-line comment. | `/* comment */` |

# Key

Keys are always treated as [Strings](#string) during parsing. Keys do
not require double quotes, however double quotes can be included for
readability. Double quotes _are_ required for keys that include any whitespace
character, although whitespace in keys should be avoided whenever possible.

## Examples

### Valid

* `hello`
* `"hello"`
    * The double quotes will be ignored during parsing
    * The double quotes _will not_ be preserved when writing
* `"hello world"`
    * The double quotes will be ignored during parsing
    * The double quotes _will_ be preserved when writing
* `"\"hello\""`
    * The inner, escaped double quotes will be parsed
    * The inner and outer double quotes _will_ be preserved when writing
* `" "`
    * The double quotes will be ignored during parsing
    * The double quotes _will_ be preserved when writing
* `1`
    * All text in the `key` position will be treated as a [String](#string)

### Invalid

* `{}`
    * A [Dictionary](#dictionary) can never exist in the `key` position unless
    it is surrounded by double quotes
* `[]`
    * A [List](#list) can never exist in the `key` position unless it is
    surrounded by double quotes
* `()`
    * An [Expression](#expression) can never exist in the `key` position unless
    it is surrounded by double quotes

# Value

A value is any of the following:

* [Unit](#unit)
* [Boolean](#boolean)
* [Integer](#integer)
* [Float](#float)
* [String](#string)
* [List](#list)
* [Dictionary](#dictionary)
* [Expression](#expression)

# Unit

A unit value that represents the absence of a value denoted by the text `null`.
This is useful for cases where only the `key` is important.

## Examples

```clojure
my-key null
my-dict {
    person-a null
    person-b null
}
```

# Boolean

A true or false value denoted by `true` or `false`. `true` and `false` are
case-sensitive.

## Examples

```clojure
is-boolean true
is-string false
some-bools [
    true
    false
    false
    true
]
```

# Integer

A whole number. An integer cannot contain any decimals otherwise it will be
treated as a [Float](#float). Integers can contain underscores for
readability, however the underscores will _not_ be preserved when writing.

## Examples

```clojure
my-int 1
other-int 100_000
```

# Float

A floating point number. A float _must_ contain a decimal in order to denote it
as a float. Floats can contain underscores for readability, however the
underscores will _not_ be preserved when writing.

Tot makes no guarantees about floating point error when reading or writing.

## Examples

```clojure
my-float 1.0
other-float 1.
decimal-only .1
bigger-float 100_000.01
```

# String

Text surrounded by double quotes. Only double quotes are allowed to denote
Strings. Escaped characters are handled as appropriate.

## Examples

```clojure
my-string "hello"
escaped "\"hello\" world\n\t"
empty-string ""
```

# List

A collection of values. [Keys](#key) are not allowed. [Unit](#unit) values
are ignored and are _not_ preserved when writing.

Newlines are encouraged for readability and are inserted by default when
writing.

Commas are _not_ required but can be used for readability. Commas _will_ be
ignored when writing.

The entire file can be denoted as a list if the entire file is wrapped in
"`[`" and "`]`" characters.

## Examples

### General

```clojure
int-list [
    1, // The comma will be ignored when reading and writing
    2
    3
]
mixed-list [
    1
    false
    "hello"
]
// This will be expanded when writing
compact-list [true false]
nested-list [
    [
        1
    ]
    [
        2
        [
            3
        ]
    ]
    {
        key "value"
    }
    (+ 1 1)
]
```

### List file

```clojure
[
    "this entire file"
    "is a list"
    {
        msg "key value pairs must be placed inside dictionaries"
        continued "key value pairs outside of the list will break parsing"
    }
]
```

# Dictionary

A collection of key-value pairs. By default, `tot` files are considered to be
Dictionaries.

Newlines are encouraged for readability and are inserted by default when
writing.

Commas are _not_ required but can be used for readability. Commas _will_ be
ignored when writing.

`tot` files _should not_ wrap the entire file in "`{`" and "`}`" characters. If
they are present, this will break parsing.

## Example

```clojure
my-key "my-value"
my-dict {
    name "tot"
}
other-dict {
    name "tot" // Keys do not clash since they are in different dictionaries
}
nested {
    inner {
        name "tot"
    }
}
commas {
    unit null, // Commas are ignored and are not preserved
    hello "world"
}
// Dictionaries will be expanded when writing
compact {key "value" int 1 bool true}
```

# Expression

A limited, deterministic, non-Turing-complete
[Lisp](https://en.wikipedia.org/wiki/Lisp_(programming_language)) usable in Tot.

Expressions are _not_ preserved when writing `tot` files.

All expressions will return a [Value](#value). Varargs are _not_ supported.
Unless specified otherwise, all values in an expression must be of the same
type.

## Addition

`(+ value1 value2)`

Add two values together.

### Valid types

* Integer
* Float
* String

### Example

* `(+ 1 1)` == `2`
* `(+ 1 (+ 1 1))` == `3`
* `(+ "hello " "world")` == `"hello world"`

## Subtraction

`(- value1 value2)`

Subtract `value2` from `value1`.

### Valid types

* Integer
* Float

### Example

* `(- 1 1)` == `0`
* `(- 1 (- 1 1))` == `1`

## Multiplication

`(* value1 value2)`

Multiply two values together.

### Valid types

* Integer
* Float

### Example

* `(* 2 2)` == `4`
* `(* 2 (* 10 1))` == `20`

## Division

`(/ value1 value2)`

Divide `value1` by `value2`.

### Valid types

* Integer
* Float

### Example

* `(/ 4 2)` == `2`
* `(/ 10 (/ 4 2))` == `5`

## Reference

`(& key-path)`

Looks up the value at `key-path` by walking the `key`s and taking the
`value` at the final `key`.

When referring to [List](#list) values, indexing starts at 0.

### Valid types

All `value`s are valid for references.

### Example

```clojure
author "me :)"
version {
    major 1
    minor 10
    patch 100
}
favorite-ints [
    2
    100
]
nested [
    {
        secret "potato"
    }
]
app-config {
    min-patch-version (& version patch) // Resolves to 100
    primary-maintainer (& author) // Resolves to "me :)"
    some-int (& favorite-ints 0) // Resolves to 2
    favorite-food (& nested 0 secret) // Resolves to "potato"
}
```

## Generator

Generators are a shortcut for creating lots of data. Generators _must_ be
defined at the root level of the file and are invalid when defined in Tot
[List](#list) files.

`(gen name [params] value)`

* `gen` is a required keyword
* `name` can be whatever the developer defines
    * Generator names cannot collide with other built-in expressions
* `params` is a list of parameter names that can be used when calculating
`value`
* `value` is what is returned by the generator and must be a [Value](#value)

Generators are invoked via the defined `name` and passing parameters as
necessary. Generators _cannot_ be invoked inside other generators, but the
output of a generator can be passed as a parameter to another generator.

Generators are:

* Shorthand for writing config files
* Simple to write and understand

Generators are _not_:

* Logic
* Meant to be complicated

If a generator starts to get large and unwieldy, it might be wise to rethink
how an application should be handling configs.

### Example

Simple:

```clojure
(gen simple [] 1)

// Resolves to: my-integer 1
my-integer (simple)
```

No params:

```clojure
// This could easily be a reference expression, but this is also valid
(gen version [] {
    major 1
    minor 0
    patch 0
})

// Resolves to: config-version {major 1 minor 0 patch 0}
config-version (version)

server1 {
    name "east"
    software-version (version)
}
server2 {
    name "west"
    software-version (version)
}
```

With params:

```clojure
(gen version [patch] {
    major 1
    minor 0
    patch patch
})

blue {
    status "active"
    // Resolves to: version {major 1 minor 0 patch 0}
    version (version 0)
}
green {
    status "inactive"
    // Resolves to: version {major 1 minor 0 patch 1}
    version (version 1)
}
```

Inner expression:

```clojure
(gen square [x] (* x x))

// Resolves to: 4squared 16
4squared (square 4)
```

Dictionary parameter:

```clojure
(gen version [v1 v2 v3] {
    major v1
    minor v2
    patch v3
})

(gen server [name env version] {
    name name
    environment env
    version version
})

server-list [
    /*
    Resolves to:
    {
        name "dev1"
        environment "dev"
        version {
            major 1
            minor 0
            patch 0
        }
    }
    */
    (server "dev1" "dev" (version 1 0 0))
    (server "dev2" "dev" (version 1 2 3))
    (server "qa1" "qa" (version 1 0 0))
]
```

Nested expression:

```clojure
(gen esteemed [name] (+ "The esteemed " (+ name " has arrived!")))

// Resolves to: maintainer "The esteemed maintainer has arrived!"
maintainer (esteemed "maintainer")
```
