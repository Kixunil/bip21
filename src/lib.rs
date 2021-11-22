//! Rust-idiomatic, compliant, flexible and performant BIP21 crate.
//!
//! **Important:** while lot of work went into polishing the crate it's still considered
//! early-development!
//!
//! * Rust-idiomatic: uses strong types, standard traits and other things
//! * Compliant: implements all requirements of BIP21, including protections to not forget about
//!              `req-`. (But see features.)
//! * Flexible: enables parsing/serializing additional arguments not defined by BIP21
//! * Performant: uses zero-copy deserialization and lazy evaluation wherever possible.
//!
//! Serialization and deserialization is inspired by `serde` with these important differences:
//!
//! * Deserialization signals if the field is known so that `req-` fields can be rejected.
//! * Much simpler API - we don't need all the features.
//! * Use of [`Param<'a>`] to enable lazy evaluation.
//!
//! The crate is `no_std` but does require `alloc`.
//!
//! ## Features
//!
//! * `std` enables integration with `std` - mainly `std::error::Error`.
//! * `non-compliant-bytes` - enables use of non-compliant API that can parse non-UTF-8 URI values.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![no_std]
#![deny(unused_must_use)]
#![deny(missing_docs)]

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

pub mod de;
pub mod ser;

use alloc::borrow::ToOwned;
use alloc::borrow::Cow;
#[cfg(feature = "non-compliant-bytes")]
use alloc::vec::Vec;
use alloc::string::String;
use percent_encoding_rfc3986::{PercentDecode, PercentDecodeError};
#[cfg(feature = "non-compliant-bytes")]
use either::Either;
use core::convert::{TryFrom, TryInto};

pub use de::{DeserializeParams, DeserializationState, DeserializationError};
pub use ser::{SerializeParams};

/// Parsed BIP21 URI.
///
/// This struct represents all fields of BIP21 URI with the ability to add more extra fields using
/// the `extras` field. By default there are no extra fields so an empty implementation is used.
#[non_exhaustive]
pub struct Uri<'a, Extras = NoExtras> {
    /// The address provided in the URI.
    ///
    /// This field is mandatory because the address is mandatory in BIP21.
    pub address: bitcoin::Address,

    /// Number of satoshis requested as payment.
    pub amount: Option<bitcoin::Amount>,

    /// The label of the address - e.g. name of the receiver.
    pub label: Option<Param<'a>>,

    /// Message that describes the transaction to the user.
    pub message: Option<Param<'a>>,

    /// Extra fields that can occur in a BIP21 URI.
    pub extras: Extras,
}

impl<'a, T> Uri<'a, T> {
    /// Creates an URI with defaults.
    ///
    /// This sets all fields except `address` to default values.
    /// They can be overwritten in subsequent assignments before displaying the URI.
    pub fn new(address: bitcoin::Address) -> Self where T: Default {
        Uri {
            address,
            amount: None,
            label: None,
            message: None,
            extras: Default::default(),
        }
    }

    /// Creates an URI with defaults.
    ///
    /// This sets all fields except `address` and `extras` to default values.
    /// They can be overwritten in subsequent assignments before displaying the URI.
    pub fn with_extras(address: bitcoin::Address, extras: T) -> Self {
        Uri {
            address,
            amount: None,
            label: None,
            message: None,
            extras,
        }
    }
}

/// Abstrated stringly parameter in the URI.
///
/// This type abstracts the parameter that may be encoded allowing lazy decoding, possibly even
/// without allocation.
/// When constructing [`Uri`] to be displayed you may use `From<S>` where `S` is one of various
/// stringly types. The conversion is always cheap.
#[derive(Clone)]
pub struct Param<'a>(ParamInner<'a>);

impl<'a> Param<'a> {
    /// Convenience constructor.
    fn decode(s: &'a str) -> Result<Self, PercentDecodeError> {
        Ok(Param(ParamInner::EncodedBorrowed(percent_encoding_rfc3986::percent_decode_str(s)?)))
    }

    /// Creates a byte iterator yielding decoded bytes.
    #[cfg(feature = "non-compliant-bytes")]
    #[cfg_attr(docsrs, doc(cfg(feature = "non-compliant-bytes")))]
    pub fn bytes(&self) -> ParamBytes<'_> {
        ParamBytes(match &self.0 {
            ParamInner::EncodedBorrowed(decoder) => Either::Left(decoder.clone()),
            ParamInner::UnencodedBytes(bytes) => Either::Right(bytes.iter().cloned()),
            ParamInner::UnencodedString(string) => Either::Right(string.as_bytes().iter().cloned()),
        })
    }

    /// Converts the parameter into iterator yielding decoded bytes.
    #[cfg(feature = "non-compliant-bytes")]
    #[cfg_attr(docsrs, doc(cfg(feature = "non-compliant-bytes")))]
    pub fn into_bytes(self) -> ParamBytesOwned<'a> {
        ParamBytesOwned(match self.0 {
            ParamInner::EncodedBorrowed(decoder) => Either::Left(decoder),
            ParamInner::UnencodedBytes(Cow::Borrowed(bytes)) => Either::Right(Either::Left(bytes.iter().cloned())),
            ParamInner::UnencodedBytes(Cow::Owned(bytes)) => Either::Right(Either::Right(bytes.into_iter())),
            ParamInner::UnencodedString(Cow::Borrowed(string)) => Either::Right(Either::Left(string.as_bytes().iter().cloned())),
            ParamInner::UnencodedString(Cow::Owned(string)) => Either::Right(Either::Right(Vec::from(string).into_iter())),
        })
    }

    /// Decodes the param if encoded making the lifetime static.
    fn decode_into_owned<'b>(self) -> Param<'b> {
        let owned = match self.0 {
            ParamInner::EncodedBorrowed(decoder) => ParamInner::UnencodedBytes(decoder.collect()),
            ParamInner::UnencodedString(Cow::Borrowed(value)) => ParamInner::UnencodedString(Cow::Owned(value.to_owned())),
            ParamInner::UnencodedString(Cow::Owned(value)) => ParamInner::UnencodedString(Cow::Owned(value)),
            ParamInner::UnencodedBytes(Cow::Borrowed(value)) => ParamInner::UnencodedBytes(Cow::Owned(value.to_owned())),
            ParamInner::UnencodedBytes(Cow::Owned(value)) => ParamInner::UnencodedBytes(Cow::Owned(value)),
        };
        Param(owned)
    }
}

/// Cheap conversion
impl<'a> From<&'a str> for Param<'a> {
    fn from(value: &'a str) -> Self {
        Param(ParamInner::UnencodedString(Cow::Borrowed(value)))
    }
}

/// Cheap conversion
impl<'a> From<String> for Param<'a> {
    fn from(value: String) -> Self {
        Param(ParamInner::UnencodedString(Cow::Owned(value)))
    }
}

/// Cheap conversion
#[cfg(feature = "non-compliant-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "non-compliant-bytes")))]
impl<'a> From<&'a [u8]> for Param<'a> {
    fn from(value: &'a [u8]) -> Self {
        Param(ParamInner::UnencodedBytes(Cow::Borrowed(value)))
    }
}

/// Cheap conversion
#[cfg(feature = "non-compliant-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "non-compliant-bytes")))]
impl<'a> From<Vec<u8>> for Param<'a> {
    fn from(value: Vec<u8>) -> Self {
        Param(ParamInner::UnencodedBytes(Cow::Owned(value)))
    }
}

/// Cheap conversion
#[cfg(feature = "non-compliant-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "non-compliant-bytes")))]
impl<'a> From<Param<'a>> for Vec<u8> {
    fn from(value: Param<'a>) -> Self {
        match value.0 {
            ParamInner::EncodedBorrowed(decoder) => decoder.collect(),
            ParamInner::UnencodedString(Cow::Borrowed(value)) => value.as_bytes().to_owned(),
            ParamInner::UnencodedString(Cow::Owned(value)) => value.into(),
            ParamInner::UnencodedBytes(value) => value.into(),
        }
    }
}

/// Cheap conversion
#[cfg(feature = "non-compliant-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "non-compliant-bytes")))]
impl<'a> From<Param<'a>> for Cow<'a, [u8]> {
    fn from(value: Param<'a>) -> Self {
        match value.0 {
            ParamInner::EncodedBorrowed(decoder) => decoder.into(),
            ParamInner::UnencodedString(Cow::Borrowed(value)) => Cow::Borrowed(value.as_bytes()),
            ParamInner::UnencodedString(Cow::Owned(value)) => Cow::Owned(value.into()),
            ParamInner::UnencodedBytes(value) => value,
        }
    }
}

impl<'a> TryFrom<Param<'a>> for String {
    type Error = core::str::Utf8Error;

    fn try_from(value: Param<'a>) -> Result<Self, Self::Error> {
        match value.0 {
            ParamInner::EncodedBorrowed(decoder) => <Cow<'_, str>>::try_from(decoder).map(Into::into),
            ParamInner::UnencodedString(value) => Ok(value.into()),
            ParamInner::UnencodedBytes(Cow::Borrowed(value)) => Ok(core::str::from_utf8(value)?.to_owned()),
            ParamInner::UnencodedBytes(Cow::Owned(value)) => String::from_utf8(value).map_err(|error| error.utf8_error()),
        }
    }
}

impl<'a> TryFrom<Param<'a>> for Cow<'a, str> {
    type Error = core::str::Utf8Error;

    fn try_from(value: Param<'a>) -> Result<Self, Self::Error> {
        match value.0 {
            ParamInner::EncodedBorrowed(decoder) => decoder.try_into(),
            ParamInner::UnencodedString(value) => Ok(value),
            ParamInner::UnencodedBytes(Cow::Borrowed(value)) => Ok(Cow::Borrowed(core::str::from_utf8(value)?)),
            ParamInner::UnencodedBytes(Cow::Owned(value)) => Ok(Cow::Owned(String::from_utf8(value).map_err(|error| error.utf8_error())?)),
        }
    }
}

#[derive(Clone)]
enum ParamInner<'a> {
    EncodedBorrowed(PercentDecode<'a>),
    UnencodedBytes(Cow<'a, [u8]>),
    UnencodedString(Cow<'a, str>),
}

/// Iterator over decoded bytes inside paramter.
///
/// The lifetime of this may be shorter than that of [`Param<'a>`].
#[cfg(feature = "non-compliant-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "non-compliant-bytes")))]
pub struct ParamBytes<'a>(ParamIterInner<'a, core::iter::Cloned<core::slice::Iter<'a, u8>>>);

/// Iterator over decoded bytes inside paramter.
///
/// The lifetime of this is same as that of [`Param<'a>`].
#[cfg(feature = "non-compliant-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "non-compliant-bytes")))]
pub struct ParamBytesOwned<'a>(ParamIterInner<'a, Either<core::iter::Cloned<core::slice::Iter<'a, u8>>, alloc::vec::IntoIter<u8>>>);

#[cfg(feature = "non-compliant-bytes")]
type ParamIterInner<'a, T> = either::Either<PercentDecode<'a>, T>;

/// Empty extras.
///
/// This type can be used if extras are not required.
/// It is also the default type parameter of [`Uri`]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NoExtras;

/// This is a state used to deserialize `NoExtras` - it doesn't expect any parameters.
#[derive(Debug, Default, Copy, Clone)]
pub struct EmptyState;

impl DeserializeParams<'_> for NoExtras {
    type DeserializationState = EmptyState;
}

impl DeserializationError for NoExtras {
    type Error = core::convert::Infallible;
}

impl<'de> DeserializationState<'de> for EmptyState {
    type Value = NoExtras;

    fn is_param_known(&self, _key: &str) -> bool {
        false
    }

    fn deserialize_temp(&mut self, _key: &str, _value: Param<'_>) -> Result<de::ParamKind, <Self::Value as DeserializationError>::Error> {
        Ok(de::ParamKind::Unknown)
    }

    fn finalize(self) -> Result<Self::Value, <Self::Value as DeserializationError>::Error> {
        Ok(Default::default())
    }
}

impl<'a> SerializeParams for &'a NoExtras {
    type Key = core::convert::Infallible;
    type Value = core::convert::Infallible;
    type Iterator = core::iter::Empty<(Self::Key, Self::Value)>;

    fn serialize_params(self) -> Self::Iterator {
        core::iter::empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::Uri;
    use alloc::string::ToString;
    use alloc::borrow::Cow;
    use core::convert::TryInto;

    fn check_send_sync<T: Send + Sync>() {}

    #[test]
    fn send_sync() {
        check_send_sync::<crate::de::UriError>();
    }

    // Note: the official test vectors contained an invalid address so it was replaced with the address of Andreas Antonopoulos.

    #[test]
    fn just_address() {
        let input = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd";
        let uri = input.parse::<Uri<'_>>().unwrap();
        assert_eq!(uri.address.to_string(), "1andreas3batLhQa2FawWjeyjCqyBzypd");
        assert!(uri.amount.is_none());
        assert!(uri.label.is_none());
        assert!(uri.message.is_none());

        assert_eq!(uri.to_string(), input);
    }

    #[test]
    fn address_with_name() {
        let input = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd?label=Luke-Jr";
        let uri = input.parse::<Uri<'_>>().unwrap();
        let label: Cow<'_, str> = uri.label.clone().unwrap().try_into().unwrap();
        assert_eq!(uri.address.to_string(), "1andreas3batLhQa2FawWjeyjCqyBzypd");
        assert_eq!(label, "Luke-Jr");
        assert!(uri.amount.is_none());
        assert!(uri.message.is_none());

        assert_eq!(uri.to_string(), input);
    }

    #[test]
    fn request_20_point_30_btc_to_luke_dash_jr() {
        // See https://github.com/rust-bitcoin/rust-bitcoin/issues/709
        // let input = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd?amount=20.3&label=Luke-Jr";
        let input = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd?amount=20.30000000&label=Luke-Jr";
        let uri = input.parse::<Uri<'_>>().unwrap();
        let label: Cow<'_, str> = uri.label.clone().unwrap().try_into().unwrap();
        assert_eq!(uri.address.to_string(), "1andreas3batLhQa2FawWjeyjCqyBzypd");
        assert_eq!(label, "Luke-Jr");
        assert_eq!(uri.amount, Some(bitcoin::Amount::from_sat(20_30_000_000)));
        assert!(uri.message.is_none());

        assert_eq!(uri.to_string(), input);
    }

    #[test]
    fn request_50_btc_with_message() {
        // See https://github.com/rust-bitcoin/rust-bitcoin/issues/709
        // let input = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz";
        let input = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd?amount=50.00000000&label=Luke-Jr&message=Donation%20for%20project%20xyz";
        let uri = input.parse::<Uri<'_>>().unwrap();
        let label: Cow<'_, str> = uri.label.clone().unwrap().try_into().unwrap();
        let message: Cow<'_, str> = uri.message.clone().unwrap().try_into().unwrap();
        assert_eq!(uri.address.to_string(), "1andreas3batLhQa2FawWjeyjCqyBzypd");
        assert_eq!(uri.amount, Some(bitcoin::Amount::from_sat(50_00_000_000)));
        assert_eq!(label, "Luke-Jr");
        assert_eq!(message, "Donation for project xyz");

        assert_eq!(uri.to_string(), input);
    }

    #[test]
    fn required_not_understood() {
        let input = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999";
        let uri = input.parse::<Uri<'_>>();
        assert!(uri.is_err());
    }

    #[test]
    fn required_understood() {
        let input = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd?somethingyoudontunderstand=50&somethingelseyoudontget=999";
        let uri = input.parse::<Uri<'_>>().unwrap();
        assert_eq!(uri.address.to_string(), "1andreas3batLhQa2FawWjeyjCqyBzypd");
        assert!(uri.amount.is_none());
        assert!(uri.label.is_none());
        assert!(uri.message.is_none());
    }
}
