// SPDX-License-Identifier: MIT OR Apache-2.0
//! The `nfc` feature (CONTRACT D4): Unicode normalisation tables live in `unicode-normalization`, so the zero-dependency
//! core only uses them when this feature is on.

use unicode_normalization::UnicodeNormalization;

/// Whether `text` is already in Unicode normalisation form C.
#[must_use]
pub fn is_nfc(text: &str) -> bool {
    unicode_normalization::is_nfc(text)
}

/// `text` in normalisation form C. Normalise before `put`; the store refuses text that is not NFC.
#[must_use]
pub fn normalise_nfc(text: &str) -> String {
    text.nfc().collect()
}

/// The byte offset of the first byte of `text` that differs from its NFC form (`text.len()` is never returned: the
/// caller has already established that `text` is not NFC).
pub(crate) fn first_difference(text: &str) -> usize {
    let nfc = normalise_nfc(text);
    text.bytes()
        .zip(nfc.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| text.len().min(nfc.len()))
}
