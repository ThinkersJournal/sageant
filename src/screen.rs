// SPDX-License-Identifier: MIT OR Apache-2.0
//! The secret-screening hook (CONTRACT D9).
//!
//! SageAnt defines only this thin trait. Detection (patterns, entropy) is RedactAnt's to design; until it
//! exists, the `redactant` feature screens for secrets the caller has registered.

/// Screens text on its way into the store. Returns the match class, never the text or its position.
pub trait SecretScreen {
    /// `Some(class)` when `text` must be refused as containing a secret.
    fn screen(&self, text: &str) -> Option<&'static str>;
}

/// Screens for exact secrets the caller has registered, using RedactAnt's `Scrubber`.
#[cfg(feature = "redactant")]
#[derive(Debug, Default)]
pub struct RegisteredSecrets(::redactant::Scrubber);

#[cfg(feature = "redactant")]
impl RegisteredSecrets {
    /// No secrets registered yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a secret to refuse.
    ///
    /// # Errors
    /// RedactAnt's `RegisterError` for an empty, too-short (under 4 bytes) or non-UTF-8 secret.
    pub fn register(
        &mut self,
        secret: &::redactant::Secret<String>,
    ) -> Result<(), ::redactant::RegisterError> {
        self.0.register(secret)
    }
}

#[cfg(feature = "redactant")]
impl SecretScreen for RegisteredSecrets {
    fn screen(&self, text: &str) -> Option<&'static str> {
        self.0.appears_in(text).then_some("registered-secret")
    }
}
