//! Types and traits related to deserialization (parsing) of BIP21
//!
//! This module provides mainly the infrastructure required to parse extra BIP21 arguments.
//! It's inspired by `serde` with main differences being handling of `req-` arguments and
//! simplicity.
//!
//! Check [`DeserializeParams`] to get started.

use alloc::borrow::ToOwned;
use alloc::borrow::Cow;
use alloc::string::String;
use core::convert::{TryFrom, TryInto};
use bitcoin::amount::{Denomination, ParseAmountError};
use bitcoin::address::ParseError as AddressError;
use bitcoin::address::NetworkValidation;
use core::fmt;
use super::{Uri, Param};
use percent_encoding_rfc3986::PercentDecodeError;

impl<'a, T: DeserializeParams<'a>> Uri<'a, bitcoin::address::NetworkUnchecked, T> {
    /// Implements deserialization.
    fn deserialize_raw(string: &'a str) -> Result<Self, Error<T::Error>> {
        const SCHEME: &str = "bitcoin:";
        if string.len() < SCHEME.len() {
            return Err(Error::Uri(UriError(UriErrorInner::TooShort)));
        }

        if !string[..SCHEME.len()].eq_ignore_ascii_case(SCHEME) {
            return Err(Error::Uri(UriError(UriErrorInner::InvalidScheme)));
        }

        let string = &string[SCHEME.len()..];

        let (address, params) = match string.find('?') {
            Some(pos) => (&string[..pos], Some(&string[(pos + 1)..])),
            None => (string, None),
        };

        let address = address.parse().map_err(Error::uri)?;
        let mut deserializer = T::DeserializationState::default();
        let mut amount = None;
        let mut label = None;
        let mut message = None;
        if let Some(params) = params {
            for param in params.split('&') {
                let pos = param
                    .find('=')
                    .ok_or_else(|| Error::Uri(UriError(UriErrorInner::MissingEquals(param.to_owned()))))?;
                let key = &param[..pos];
                let value = &param[(pos + 1)..];
                match key {
                    "amount" => {
                        let parsed_amount = bitcoin::Amount::from_str_in(value, Denomination::Bitcoin).map_err(Error::uri)?;
                        amount = Some(parsed_amount);
                    },
                    "label" => {
                        let label_decoder = Param::decode(value).map_err(Error::percent_decode_static("label"))?;
                        label = Some(label_decoder);
                    },
                    "message" => {
                        let message_decoder = Param::decode(value).map_err(Error::percent_decode_static("message"))?;
                        message = Some(message_decoder);
                    },
                    extra_key => {
                        let decoder = Param::decode(value).map_err(Error::percent_decode(key))?;
                        let is_known = deserializer.deserialize_borrowed(extra_key, decoder).map_err(Error::Extras)?;
                        if is_known == ParamKind::Unknown && extra_key.starts_with("req-") {
                            return Err(Error::Uri(UriError(UriErrorInner::UnknownRequiredParameter(extra_key.to_owned()))));
                        }
                    },
                }
            }
        }
        let extras = deserializer.finalize().map_err(Error::Extras)?;

        Ok(Uri {
            address,
            amount,
            label,
            message,
            extras,
        })
    }
}

impl<'a, NetVal: NetworkValidation, T> Uri<'a, NetVal, T> {
    /// Makes the lifetime `'static` by converting all fields to owned.
    ///
    /// Note that this does **not** affect `extras`!
    fn into_static(self) -> Uri<'static, NetVal, T> {
        Uri {
            address: self.address,
            amount: self.amount,
            label: self.label.map(|label| label.decode_into_owned()),
            message: self.message.map(|message| message.decode_into_owned()),
            extras: self.extras,
        }
    }
}

/// Indicates whether a parameter with this name is known.
///
/// This is a semantically clear version of `bool` that also contains `#[must_use]`
#[must_use = "param kind MUST be checked because URI with unknown req- param MUST be rejected"]
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub enum ParamKind {
    /// Signals that this parameter is known to the type being deserialized.
    Known,

    /// Signals that this parameter is **not** known to the type being deserialized.
    ///
    /// Parsing error will be reported if this is returned from `req-` parameter as mandated by
    /// BIP21.
    Unknown,
}

/// Defines error type of deserialization.
///
/// This is a separate trait to ensure the error is same for all lifetimes.
pub trait DeserializationError {
    /// The error returned when deserialization fails.
    type Error;
}

/// Represents the state of deserialization of extras.
pub trait DeserializationState<'de>: Default {
    /// Value returned when deserialization finishes.
    type Value: DeserializationError;

    /// Returns `true` if the parameter is known.
    ///
    /// Required parameters include the `req-` prefix.
    fn is_param_known(&self, key: &str) -> bool;

    /// Deserializes a temporary.
    ///
    /// This can not borrow the key nor value, so has to clone them or throw away.
    /// Required parameters include the `req-` prefix.
    fn deserialize_temp(&mut self, key: &str, value: Param<'_>) -> Result<ParamKind, <Self::Value as DeserializationError>::Error>;

    /// Deserializes a borrowed value possibly avoiding cloning.
    ///
    /// Implementing this can enable zero-copy deserialization.
    /// Required parameters include the `req-` prefix.
    ///
    /// The default implementation forwards to `deserialize_temp`
    fn deserialize_borrowed(&mut self, key: &'de str, value: Param<'de>) -> Result<ParamKind, <Self::Value as DeserializationError>::Error> {
        self.deserialize_temp(key, value)
    }

    /// Signals that all parameters were processed.
    ///
    /// This function may perform additional validation - e.g. checking if some mandatory fields are missing.
    fn finalize(self) -> Result<Self::Value, <Self::Value as DeserializationError>::Error>;
}

/// Represents a value that can be deserialized.
///
/// All values passed in `Extras` type parameter of [`Uri`] must implement this trait to allow
/// deserialization.
pub trait DeserializeParams<'de>: Sized + DeserializationError {
    /// State used when deserializing.
    type DeserializationState: DeserializationState<'de, Value = Self>;
}

/// Error returned when parsing URI.
#[derive(Clone, Debug)]
pub enum Error<T> {
    /// Parsing of BIP21 URI failed.
    ///
    /// This reports failures related to BIP21 requirements including parse error for address.
    Uri(UriError),
    /// Parsing of extras failed.
    ///
    /// This only directly forwards parsing error from extras.
    Extras(T),
}

impl<T> Error<T> {
    fn uri<U: Into<UriErrorInner>>(error: U) -> Self {
        Error::Uri(UriError(error.into()))
    }

    fn percent_decode_static(parameter: &'static str) -> impl FnOnce(PercentDecodeError) -> Self {
        move |error| {
            Self::uri(UriErrorInner::PercentDecode {
                parameter: Cow::Borrowed(parameter),
                error,
            })
        }
    }

    fn percent_decode(parameter: &str) -> impl '_ + FnOnce(PercentDecodeError) -> Self {
        move |error| {
            Self::uri(UriErrorInner::PercentDecode {
                parameter: parameter.to_owned().into(),
                error,
            })
        }
    }
}

impl<T: fmt::Display> fmt::Display for Error<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Uri(_) => write!(f, "invalid BIP21 URI"),
            Error::Extras(_) => write!(f, "failed to parse extra argument(s)"),
        }
    }
}

#[cfg(feature = "std")]
impl<T: fmt::Display + std::error::Error + 'static> std::error::Error for Error<T> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Uri(error) => Some(error),
            Error::Extras(error) => Some(error),
        }
    }
}

/// Error returned when parsing non-extras parts of URI.
#[derive(Debug, Clone)]
pub struct UriError(UriErrorInner);

#[derive(Debug, Clone)]
enum UriErrorInner {
    TooShort,
    InvalidScheme,
    Address(AddressError),
    Amount(ParseAmountError),
    UnknownRequiredParameter(String),
    PercentDecode {
        parameter: Cow<'static, str>,
        error: PercentDecodeError,
    },
    MissingEquals(String),
}

impl From<AddressError> for UriErrorInner {
    fn from(value: AddressError) -> Self {
        UriErrorInner::Address(value)
    }
}

impl From<ParseAmountError> for UriErrorInner {
    fn from(value: ParseAmountError) -> Self {
        UriErrorInner::Amount(value)
    }
}

impl fmt::Display for UriError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.0 {
            UriErrorInner::TooShort => write!(f, "the URI is too short"),
            UriErrorInner::InvalidScheme => write!(f, "the URI has invalid scheme"),
            UriErrorInner::Address(_) => write!(f, "the address is invalid"),
            UriErrorInner::Amount(_) => write!(f, "the amount is invalid"),
            UriErrorInner::UnknownRequiredParameter(parameter) => write!(f, "the URI contains unknown required parameter '{}'", parameter),
            #[cfg(feature = "std")]
            UriErrorInner::PercentDecode { parameter, error: _ } => write!(f, "can not percent-decode parameter {}", parameter),
            #[cfg(not(feature = "std"))]
            UriErrorInner::PercentDecode { parameter, error } => write!(f, "can not percent-decode parameter {}: {}", parameter, error),
            UriErrorInner::MissingEquals(parameter) => write!(f, "the parameter '{}' is missing a value", parameter),
        }
    }
}

#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
impl std::error::Error for UriError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.0 {
            UriErrorInner::TooShort => None,
            UriErrorInner::InvalidScheme => None,
            UriErrorInner::Address(error) => Some(error),
            UriErrorInner::Amount(error) => Some(error),
            UriErrorInner::UnknownRequiredParameter(_) => None,
            UriErrorInner::PercentDecode { parameter: _, error } => Some(error),
            UriErrorInner::MissingEquals(_) => None,
        }
    }
}

/// **Warning**: this implementation may needlessly allocate, consider using `TryFrom<&str>` instead.
impl<'a, T: for<'de> DeserializeParams<'de>> core::str::FromStr for Uri<'a, bitcoin::address::NetworkUnchecked, T> {
    type Err = Error<T::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uri::deserialize_raw(s).map(Uri::into_static)
    }
}

impl<'a, T: DeserializeParams<'a>> TryFrom<&'a str> for Uri<'a, bitcoin::address::NetworkUnchecked, T> {
    type Error = Error<T::Error>;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        Self::deserialize_raw(s)
    }
}

/// **Warning**: this implementation may needlessly allocate, consider using `TryFrom<&str>` instead.
impl<'a, T: for<'de> DeserializeParams<'de>> TryFrom<String> for Uri<'a, bitcoin::address::NetworkUnchecked, T> {
    type Error = Error<T::Error>;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// **Warning**: this implementation may needlessly allocate, consider using `TryFrom<&str>` instead.
impl<'a, T: for<'de> DeserializeParams<'de>> TryFrom<Cow<'a, str>> for Uri<'a, bitcoin::address::NetworkUnchecked, T> {
    type Error = Error<T::Error>;

    fn try_from(s: Cow<'a, str>) -> Result<Self, Self::Error> {
        match s {
            Cow::Borrowed(s) => s.try_into(),
            Cow::Owned(s) => s.parse(),
        }
    }
}

impl<'a, T: DeserializeParams<'a>> Uri<'a, bitcoin::address::NetworkUnchecked, T> {
    /// Checks whether network of this address is as required.
    ///
    /// For details about this mechanism, see section [*parsing addresses*](bitcoin::Address#parsing-addresses) on [`bitcoin::Address`].
    pub fn require_network(self, network: bitcoin::Network) -> Result<Uri<'a, bitcoin::address::NetworkChecked, T>, Error<T::Error>> {
        let address = self.address.require_network(network).map_err(Error::uri)?;
        Ok(Uri {
            address,
            amount: self.amount,
            label: self.label,
            message: self.message,
            extras: self.extras,
        })
    }

    /// Marks URI validated without checks.
    pub fn assume_checked(self) -> Uri<'a, bitcoin::address::NetworkChecked, T> {
        Uri {
            address: self.address.assume_checked(),
            amount: self.amount,
            label: self.label,
            message: self.message,
            extras: self.extras,
        }
    }
}
