//! Environment-sourced configuration for cloud localization.
//!
//! All `std::env` reads live here, parsed into named types.

/// Roblox cloud localization auth token sourced from `LEXI_AUTH_TOKEN`.
pub struct LexiAuthToken(String);

impl LexiAuthToken {
    pub fn from_env() -> Option<Self> {
        match std::env::var("LEXI_AUTH_TOKEN") {
            Ok(token) => Some(Self(token)),
            Err(_) => None,
        }
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Roblox universe ID sourced from `ROBLOX_UNIVERSE_ID`.
pub struct UniverseId(u64);

impl UniverseId {
    pub fn from_env() -> Option<Self> {
        match std::env::var("ROBLOX_UNIVERSE_ID") {
            Ok(raw) => match raw.parse::<u64>() {
                Ok(id) => Some(Self(id)),
                Err(_) => None,
            },
            Err(_) => None,
        }
    }

    pub fn into_inner(self) -> u64 {
        self.0
    }
}
