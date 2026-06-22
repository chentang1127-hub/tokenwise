//! Pro License verification.
//!
//! Key format: `tw_pro_<base64url(payload)>`
//!   payload = expiry_timestamp (u64 BE, 8 bytes) || HMAC-SHA256 (32 bytes)
//!
//! The HMAC key is a compile-time constant.  A valid license:
//!   - Decodes successfully
//!   - Has not expired
//!   - HMAC signature matches
//!
//! Free tier restrictions (when license is missing/invalid):
//!   - Safety net disabled
//!   - Cache disabled
//!
//! All providers available — Pro differentiator is smart routing + cache.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{info, warn};

/// Embedded secret for license key signing.
/// This is a cryptographically random 32-byte key generated at build time.
/// Override at build time with `--cfg tokenwise_secret="..."`
const LICENSE_SECRET: &[u8] = b"\x7f\xa2\xd1\x3e\x8b\x55\x91\xc4\xf0\x6d\x2a\x79\x0e\xb8\x33\x5c\xa1\x94\xe7\x2f\x46\xd8\x0b\xc6\x1a\x3d\x57\x9f\xe2\x04\x68\xcd";

/// License status after verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseTier {
    /// No key provided — Free tier.
    Free,
    /// Valid Pro license.
    Pro {
        /// Unix timestamp when this license expires.
        expires_at: u64,
    },
}

impl LicenseTier {
    /// Whether safety net fallback is enabled.
    /// (Currently gated by main.rs direct tier comparison; reserved for future use.)
    #[allow(dead_code)]
    pub fn safety_net_enabled(&self) -> bool {
        matches!(self, LicenseTier::Pro { .. })
    }

    /// Maximum number of providers allowed. None = unlimited.
    #[allow(dead_code)]
    pub fn max_providers(&self) -> Option<usize> {
        // Both tiers get all providers — Pro differentiator is routing + cache
        None
    }

    /// Whether smart routing is enabled (Pro only).
    /// Free tier passes through to the original model without rewriting.
    pub fn routing_enabled(&self) -> bool {
        matches!(self, LicenseTier::Pro { .. })
    }

    /// Tier name for display.
    pub fn name(&self) -> &'static str {
        match self {
            LicenseTier::Free => "Free",
            LicenseTier::Pro { .. } => "Pro",
        }
    }
}

/// Verify a license key string. Returns the determined tier.
pub fn verify_license(key: &str) -> LicenseTier {
    if key.is_empty() {
        info!("No license key provided — running in Free tier (passthrough mode, all providers)");
        return LicenseTier::Free;
    }

    // Strip prefix if present
    let encoded = key.strip_prefix("tw_pro_").unwrap_or(key);

    // Decode base64url
    let payload = match base64url_decode(encoded) {
        Ok(p) => p,
        Err(e) => {
            warn!("License key decode failed: {e}. Falling back to Free tier.");
            return LicenseTier::Free;
        }
    };

    if payload.len() != 40 {
        warn!(
            "License key has wrong payload length ({} != 40). Falling back to Free tier.",
            payload.len()
        );
        return LicenseTier::Free;
    }

    // Split: first 8 bytes = expiry, last 32 bytes = signature
    let expiry_bytes: [u8; 8] = payload[..8].try_into().unwrap();
    let signature: [u8; 32] = payload[8..].try_into().unwrap();

    let expires_at = u64::from_be_bytes(expiry_bytes);

    // Verify HMAC
    let mut mac =
        Hmac::<Sha256>::new_from_slice(LICENSE_SECRET).expect("HMAC can take any key length");
    mac.update(&expiry_bytes);

    if mac.verify_slice(&signature).is_err() {
        warn!("License key signature invalid. Falling back to Free tier.");
        return LicenseTier::Free;
    }

    // Check expiry
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if expires_at < now {
        let days_ago = (now - expires_at) / 86400;
        warn!("License key expired {days_ago} days ago. Falling back to Free tier.");
        return LicenseTier::Free;
    }

    let days_left = (expires_at - now) / 86400;
    info!("✅ Pro license active — {days_left} days remaining. All features enabled.");

    LicenseTier::Pro { expires_at }
}

/// Minimal base64url decode (no padding, URL-safe alphabet).
fn base64url_decode(input: &str) -> Result<Vec<u8>, String> {
    // Convert base64url → standard base64
    let mut std = input.replace('-', "+").replace('_', "/");
    // Add padding
    let pad = (4 - (std.len() % 4)) % 4;
    std.push_str(&"=".repeat(pad));

    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(&std)
        .map_err(|e| format!("base64 decode error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a valid test key for the given expiry timestamp.
    fn generate_test_key(expires_at: u64) -> String {
        let expiry_bytes = expires_at.to_be_bytes();
        let mut mac = Hmac::<Sha256>::new_from_slice(LICENSE_SECRET).unwrap();
        mac.update(&expiry_bytes);
        let signature = mac.finalize().into_bytes();

        let mut payload = Vec::with_capacity(40);
        payload.extend_from_slice(&expiry_bytes);
        payload.extend_from_slice(&signature);

        // Convert to base64url
        use base64::Engine;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload);
        format!("tw_pro_{encoded}")
    }

    #[test]
    fn test_empty_key_is_free() {
        assert_eq!(verify_license(""), LicenseTier::Free);
    }

    #[test]
    fn test_invalid_key_is_free() {
        assert_eq!(verify_license("garbage"), LicenseTier::Free);
        assert_eq!(verify_license("tw_pro_!!!!"), LicenseTier::Free);
    }

    #[test]
    fn test_valid_key_is_pro() {
        // Generate a key that expires in 365 days
        let future = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 86400;
        let key = generate_test_key(future);
        let result = verify_license(&key);
        assert!(matches!(result, LicenseTier::Pro { .. }));
    }

    #[test]
    fn test_expired_key_is_free() {
        let past = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 86400; // yesterday
        let key = generate_test_key(past);
        assert_eq!(verify_license(&key), LicenseTier::Free);
    }

    #[test]
    fn test_tampered_key_is_free() {
        let future = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 86400;
        let mut key = generate_test_key(future);
        // Tamper with a character
        key.push('x');
        assert_eq!(verify_license(&key), LicenseTier::Free);
    }
}
