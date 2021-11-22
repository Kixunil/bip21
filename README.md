# Rust implementation of BIP21

Rust-idiomatic, compliant, flexible and performant BIP21 crate.

## About

**Important:** while lot of work went into polishing the crate it's still considered
early-development!

* Rust-idiomatic: uses strong types, standard traits and other things
* Compliant: implements all requirements of BIP21, including protections to not forget about
             `req-`. (But see features.)
* Flexible: enables parsing/serializing additional arguments not defined by BIP21
* Performant: uses zero-copy deserialization and lazy evaluation wherever possible.

Serialization and deserialization is inspired by `serde` with these important differences:

* Deserialization signals if the field is known so that `req-` fields can be rejected.
* Much simpler API - we don't need all the features.
* Use of [`Param<'a>`] to enable lazy evaluation.

The crate is `no_std` but does require `alloc`.

## Features    

* `std` enables integration with `std` - mainly `std::error::Error`.
* `non-compliant-bytes` - enables use of non-compliant API that can parse non-UTF-8 URI values.

## MSRV

1.41.1

## License

MITNFA
