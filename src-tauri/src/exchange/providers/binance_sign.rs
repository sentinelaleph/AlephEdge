//! Binance request signing, one place for both key types.
//!
//! - **HMAC** keys (made on Binance, "System generated"): the `signature`
//!   parameter is the lowercase hex HMAC-SHA256 of the exact query string,
//!   keyed with the API secret.
//! - **Ed25519** keys ("Self-generated"): the app makes the keypair, the user
//!   registers the PUBLIC key in Binance API Management, and Binance returns
//!   an API key. The `signature` parameter is the base64 Ed25519 signature of
//!   the same exact payload, percent-encoded, because base64 carries `+ / =`.
//!   Source: developers.binance.com/docs/binance-spot-api-docs/rest-api/request-security
//!   (signed-endpoint examples, Ed25519 keys) and Binance's own connector,
//!   `@binance/common` `RequestSigner.sign` (one signer for spot, futures and
//!   SAPI: `crypto.sign(null, payload, ed25519Key).toString("base64")`, then
//!   URL-encoded as a query parameter).
//!
//! The Ed25519 private key is stored in the vault as the base64 of its 32-byte
//! seed, in `ExchangeCredential::api_secret`, with `key_type = Ed25519`. It is
//! never shown, logged, exported or put on the clipboard; only the public key
//! leaves this module.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signer as _, SigningKey};
use hmac::{Hmac, KeyInit, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use zeroize::Zeroizing;

use super::super::model::BinanceKeyCheckError as E;

/// How a request to Binance is signed. Borrowed from the credential for the
/// one call; `Copy` so it passes through the order layer like the `&str`
/// secret did.
#[derive(Clone, Copy)]
pub enum BinanceSigner<'a> {
    /// The API secret Binance issued.
    Hmac(&'a str),
    /// Base64 of the 32-byte Ed25519 seed, as stored in the vault.
    Ed25519(&'a str),
}

/// Hand-written so a stray `{:?}` never prints key material.
impl std::fmt::Debug for BinanceSigner<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hmac(_) => f.write_str("BinanceSigner::Hmac(<redacted>)"),
            Self::Ed25519(_) => f.write_str("BinanceSigner::Ed25519(<redacted>)"),
        }
    }
}

impl BinanceSigner<'_> {
    /// The `signature` query value for `payload` (the exact string sent),
    /// ready to append after `&signature=`.
    pub fn signature_param(&self, payload: &str) -> Result<String, E> {
        match self {
            Self::Hmac(secret) => Ok(hmac_hex(secret, payload)),
            Self::Ed25519(seed) => {
                let key = signing_key(seed).ok_or_else(|| E::InvalidCredentials {
                    code: 0,
                    msg: "Ed25519 private key unreadable".into(),
                })?;
                Ok(percent_encode_b64(&ed25519_base64(&key, payload)))
            }
        }
    }
}

/// HMAC-SHA256 as lowercase hex, Binance's standard signed-endpoint scheme.
pub(super) fn hmac_hex(secret: &str, payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts a key of any length");
    mac.update(payload.as_bytes());
    mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// Base64 (standard alphabet, padded) of the Ed25519 signature of `payload`.
fn ed25519_base64(key: &SigningKey, payload: &str) -> String {
    B64.encode(key.sign(payload.as_bytes()).to_bytes())
}

/// Percent-encodes the three base64 characters that are not URL-safe.
fn percent_encode_b64(b64: &str) -> String {
    let mut out = String::with_capacity(b64.len() + 12);
    for c in b64.chars() {
        match c {
            '+' => out.push_str("%2B"),
            '/' => out.push_str("%2F"),
            '=' => out.push_str("%3D"),
            c => out.push(c),
        }
    }
    out
}

/// The signing key from the vault's base64 seed; None if it is not 32 bytes
/// of base64. The decoded buffer is wiped on drop; `SigningKey` wipes itself.
fn signing_key(seed_b64: &str) -> Option<SigningKey> {
    let raw = Zeroizing::new(B64.decode(seed_b64.trim()).ok()?);
    let seed: Zeroizing<[u8; 32]> = Zeroizing::new(raw.as_slice().try_into().ok()?);
    Some(SigningKey::from_bytes(&seed))
}

/// DER prefix of an Ed25519 SubjectPublicKeyInfo (RFC 8410): SEQUENCE,
/// AlgorithmIdentifier { OID 1.3.101.112 }, BIT STRING of 32 bytes.
const ED25519_SPKI_PREFIX: [u8; 12] = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];

/// The public key as PEM (`BEGIN PUBLIC KEY`), the form Binance's
/// "Self-generated" key registration takes.
fn public_key_pem(key: &SigningKey) -> String {
    let mut der = ED25519_SPKI_PREFIX.to_vec();
    der.extend_from_slice(key.verifying_key().as_bytes());
    format!("-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----", B64.encode(der))
}

/// A freshly generated Ed25519 keypair waiting for the API key Binance issues
/// for its public half. The seed is wiped on drop; Debug is redacted.
pub struct Ed25519Pending {
    seed_b64: Zeroizing<String>,
    pub public_pem: String,
}

impl std::fmt::Debug for Ed25519Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ed25519Pending")
            .field("seed", &"<redacted>")
            .field("public_pem", &self.public_pem)
            .finish()
    }
}

impl Ed25519Pending {
    /// A new keypair from the OS CSPRNG.
    pub fn generate() -> Self {
        let mut seed = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(seed.as_mut());
        Self::from_seed(&seed)
    }

    fn from_seed(seed: &[u8; 32]) -> Self {
        let key = SigningKey::from_bytes(seed);
        Self { seed_b64: Zeroizing::new(B64.encode(seed)), public_pem: public_key_pem(&key) }
    }

    /// The private key in its vault form. Only the vault command that seals
    /// it reads this.
    pub fn seed_b64(&self) -> &str {
        &self.seed_b64
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sha2::Digest;

    /// Test key material derived from a label, so no key-shaped literal sits
    /// in the source.
    pub(crate) fn test_seed(label: &str) -> [u8; 32] {
        Sha256::digest(label.as_bytes()).into()
    }

    const PAYLOAD: &str =
        "symbol=BTCUSDT&side=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2&timestamp=1668481559918&recvWindow=5000";

    /// Known answer computed independently with OpenSSL 3.2.4
    /// (`openssl pkeyutl -sign -rawin` over the same payload, then base64)
    /// from the same derived seed.
    #[test]
    fn ed25519_signature_matches_an_independent_openssl_answer() {
        let pending = Ed25519Pending::from_seed(&test_seed("aleph-edge ed25519 known-answer seed"));
        assert_eq!(
            pending.public_pem,
            "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAyIDuulglZABldmoq6uP2gY67ZQzxUms3ht/bmApVssQ=\n-----END PUBLIC KEY-----"
        );
        let key = signing_key(pending.seed_b64()).expect("seed decodes");
        let b64 = ed25519_base64(&key, PAYLOAD);
        assert_eq!(b64, "Vc6ZTIXO5Law7IG8Ps2dM9zZAjVRQU8jql7fzELhVZYQst/yPo0o1X0PJH38OMY0YrWjIx17DErSuMBb/em0CQ==");
        // On the wire it is percent-encoded.
        let param = BinanceSigner::Ed25519(pending.seed_b64()).signature_param(PAYLOAD).unwrap();
        assert_eq!(param, percent_encode_b64(&b64));
        assert!(param.ends_with("%3D%3D") && param.contains("%2F") && !param.contains('/'));
    }

    #[test]
    fn the_signature_verifies_against_the_registered_public_key() {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let pending = Ed25519Pending::generate();
        let body = pending.public_pem.lines().nth(1).unwrap();
        let der = B64.decode(body).unwrap();
        assert_eq!(der[..12], ED25519_SPKI_PREFIX);
        let vk = VerifyingKey::from_bytes(der[12..].try_into().unwrap()).unwrap();
        let key = signing_key(pending.seed_b64()).unwrap();
        let sig = Signature::from_slice(&B64.decode(ed25519_base64(&key, PAYLOAD)).unwrap()).unwrap();
        assert!(vk.verify(PAYLOAD.as_bytes(), &sig).is_ok());
        assert!(vk.verify(b"timestamp=1", &sig).is_err());
    }

    #[test]
    fn two_generated_keys_differ_and_an_unreadable_seed_is_a_key_error() {
        assert_ne!(Ed25519Pending::generate().public_pem, Ed25519Pending::generate().public_pem);
        for bad in ["", "not base64!", &B64.encode([1u8; 31])] {
            assert!(matches!(
                BinanceSigner::Ed25519(bad).signature_param(PAYLOAD),
                Err(E::InvalidCredentials { code: 0, .. })
            ));
        }
    }

    #[test]
    fn debug_output_never_contains_key_material() {
        let pending = Ed25519Pending::from_seed(&test_seed("debug"));
        let seed = pending.seed_b64().to_string();
        assert!(!format!("{pending:?}").contains(&seed));
        assert!(!format!("{:?}", BinanceSigner::Ed25519(&seed)).contains(&seed));
        assert!(!format!("{:?}", BinanceSigner::Hmac("hmac-secret-x")).contains("hmac-secret-x"));
    }
}
