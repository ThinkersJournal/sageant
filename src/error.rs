// SPDX-License-Identifier: MIT OR Apache-2.0
use core::fmt;

/// Why an operation was refused. A refused write changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Text is not known to be NFC-normalised; `at` is the byte offset of the first byte that could not be verified.
    NotNormalised {
        /// Byte offset of the first non-ASCII byte.
        at: usize,
    },
    /// Content is over the collection's byte cap.
    TooLarge {
        /// Size of the refused content in bytes.
        measured: usize,
        /// The collection's cap in bytes.
        cap: usize,
    },
    /// The id or version does not exist.
    NotFound,
    /// The write would not change anything, or the expected version is not the current one.
    Conflict {
        /// The version the caller's write was based on (0 for a first write).
        expected: u32,
        /// The version found in the store.
        found: u32,
    },
    /// The text matched a secret screen. Carries the match class only, never the text or its position.
    SecretRefused {
        /// Class of the match, such as `registered-secret`.
        class: &'static str,
    },
    /// A required field is empty; names the field.
    Invalid(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotNormalised { at } => {
                write!(f, "text not verifiably NFC (first non-ASCII byte at {at})")
            }
            Self::TooLarge { measured, cap } => {
                write!(f, "content is {measured} bytes, cap is {cap}")
            }
            Self::NotFound => f.write_str("not found"),
            Self::Conflict { expected, found } => {
                write!(f, "conflict: expected version {expected}, found {found}")
            }
            Self::SecretRefused { class } => {
                write!(f, "refused: text matched a secret screen ({class})")
            }
            Self::Invalid(field) => write!(f, "empty {field}"),
        }
    }
}

impl core::error::Error for Error {}
