//! Environment-sourced configuration.
//!
//! All `std::env` reads live here, parsed into named types.

/// Roblox Open Cloud API key sourced from `TRUFFLE_API_KEY`.
pub struct ApiKey(String);

impl ApiKey {
    pub fn from_env() -> Option<Self> {
        match std::env::var("TRUFFLE_API_KEY") {
            Ok(key) => Some(Self(key)),
            Err(_) => None,
        }
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}
