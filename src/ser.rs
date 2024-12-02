//! Types and traits related to serialization (displaying) of BIP21
//!
//! This module provides mainly the infrastructure required to display extra BIP21 arguments.
//!
//! Check [`SerializeParams`] to get started.

use alloc::borrow::Cow;
use bitcoin::amount::Denomination;
use core::fmt;
use super::{Uri, Param, ParamInner};

/// Represents a value that can be serialized.
///
/// The `Extras` type parameter must implement this for [`Uri`] to be displayable.
pub trait SerializeParams {
    /// Parameter name.
    ///
    /// **Warning**: displaying [`Uri`] will panic if the key contains `=` character!
    type Key: fmt::Display;
    /// Parameter value.
    type Value: fmt::Display;

    /// Iterator over key-value pairs
    type Iterator: Iterator<Item = (Self::Key, Self::Value)>;

    /// Constructs the iterator over key-value pairs.
    fn serialize_params(self) -> Self::Iterator;
}

/// Checks if the display implementation outputs `=` character.
struct EqSignChecker<'a, W: fmt::Write>(W, &'a dyn fmt::Display);

impl<W: fmt::Write> fmt::Write for EqSignChecker<'_, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if s.contains('=') {
            panic!("key '{}' contains equal sign", self.1);
        }
        self.0.write_str(s)
    }

    fn write_char(&mut self, c: char) -> fmt::Result {
        if c == '=' {
            panic!("key '{}' contains equal sign", self.1);
        }
        self.0.write_char(c)
    }
}

/// Set of characters that will be percent-encoded
///
/// This contains anything not in `query` (i.e. ``gen-delim` from the quoted
/// definitions`) as per RFC 3986, as well as '&' and '=' as per BIP 21.
///
/// [BIP 21](https://github.com/bitcoin/bips/blob/master/bip-0021.mediawiki#abnf-grammar):
///
/// > ```text
/// > labelparam     = "label=" *qchar
/// > messageparam   = "message=" *qchar
/// > otherparam     = qchar *qchar [ "=" *qchar ]
/// > ```
/// ...
/// > Here, "qchar" corresponds to valid characters of an RFC 3986 URI query
/// > component, excluding the "=" and "&" characters, which this BIP takes as
/// > separators.
///
/// [RFC 3986 Appendix A](https://www.rfc-editor.org/rfc/rfc3986#appendix-A):
///
/// > ```text
/// > pchar         = unreserved / pct-encoded / sub-delims / ":" / "@"
/// > query         = *( pchar / "/" / "?" )
/// > ```
/// ...
/// > ```text
/// > pct-encoded   = "%" HEXDIG HEXDIG
/// > unreserved    = ALPHA / DIGIT / "-" / "." / "_" / "~"
/// > ```
/// ...
/// > ```text
/// > sub-delims    = "!" / "$" / "&" / "'" / "(" / ")"
/// >               / "*" / "+" / "," / ";" / "="
/// > ```
const ASCII_SET: percent_encoding_rfc3986::AsciiSet = percent_encoding_rfc3986::NON_ALPHANUMERIC
    // allow non-alphanumeric characters from `unreserved`
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    // allow non-alphanumeric characters from `sub-delims` excluding bip-21
    // separators ("&", and "=")
    .remove(b'!')
    .remove(b'$')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b';')
    // allow pchar extra chars
    .remove(b':')
    .remove(b'@')
    // allow query extra chars
    .remove(b'/')
    .remove(b'?');

/// Percent-encodes writes.
struct WriterEncoder<W: fmt::Write>(W);

impl<W: fmt::Write> fmt::Write for WriterEncoder<W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write!(self.0, "{}", percent_encoding_rfc3986::utf8_percent_encode(s, &ASCII_SET))
    }
}

/// Percent-encodes `Display` impl.
struct DisplayEncoder<T: fmt::Display>(T);

impl<T: fmt::Display> fmt::Display for DisplayEncoder<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use fmt::Write;

        write!(WriterEncoder(f), "{}", self.0)
    }
}

/// Displays [`Param`] as encoded
///
/// This is private because people should generally only display values as decoded
struct DisplayParam<'a>(&'a Param<'a>);

impl fmt::Display for DisplayParam<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &(self.0).0 {
            // TODO: improve percent_encoding_rfc_3986 so that allocation can be avoided
            ParamInner::EncodedBorrowed(decoder) => {
                let decoded = <Cow<'_, [u8]>>::from(decoder.clone());
                write!(f, "{}", percent_encoding_rfc3986::percent_encode(&decoded, &ASCII_SET))
            },
            ParamInner::UnencodedBytes(bytes) => write!(f, "{}", percent_encoding_rfc3986::percent_encode(bytes, &ASCII_SET)),
            ParamInner::UnencodedString(string) => write!(f, "{}", percent_encoding_rfc3986::utf8_percent_encode(string, &ASCII_SET)),
        }
    }
}

/// Writes key-value pair with all required symbols around them.
///
/// `value` is **not** percent-encoded - this must be done from the caller.
fn write_param(writer: &mut impl fmt::Write, key: impl fmt::Display, value: impl fmt::Display, no_params: &mut bool) -> fmt::Result {
    use core::fmt::Write;

    if *no_params {
        write!(EqSignChecker(&mut *writer, &key), "?{}", key)?;
        *no_params = false;
    } else {
        write!(EqSignChecker(&mut *writer, &key), "&{}", key)?;
    }
    write!(writer, "={}", value)
}

/// Write key-value pair if `value` is `Some`.
fn maybe_write_param(writer: &mut impl fmt::Write, key: impl fmt::Display, value: Option<&Param<'_>>, no_params: &mut bool) -> fmt::Result {
    match value {
        Some(value) => write_param(writer, key, DisplayParam(value), no_params),
        None => Ok(()),
    }
}

/// Write key-value pair if `value` is `Some`.
fn maybe_display_param(writer: &mut impl fmt::Write, key: impl fmt::Display, value: Option<impl fmt::Display>, no_params: &mut bool) -> fmt::Result {
    match value {
        Some(value) => write_param(writer, key, DisplayEncoder(value), no_params),
        None => Ok(()),
    }
}

/// Formats QR-code-optimized URI if alternate form (`{:#}`) is used.
#[rustfmt::skip]
impl<T> fmt::Display for Uri<'_, bitcoin::address::NetworkChecked, T> where for<'a> &'a T: SerializeParams {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if f.alternate() {
            write!(f, "bitcoin:{:#}", self.address)?;
        } else {
            write!(f, "bitcoin:{}", self.address)?;
        }
        let mut no_params = true;
        let display_amount = self.amount.as_ref().map(|amount| amount.display_in(Denomination::Bitcoin));

        maybe_display_param(f, "amount", display_amount, &mut no_params)?;
        maybe_write_param(f, "label", self.label.as_ref(), &mut no_params)?;
        maybe_write_param(f, "message", self.message.as_ref(), &mut no_params)?;

        for (key, value) in self.extras.serialize_params() {
            write_param(f, key, DisplayEncoder(value), &mut no_params)?;
        }
        Ok(())
    }
}
