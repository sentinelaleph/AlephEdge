//! Vault cryptographic primitives: Argon2id key derivation and AES-256-GCM
//! authenticated encryption. This module holds no state and never logs inputs.
//!
//! Errors are stable codes the UI localizes (`errors.*`). A failed AEAD open is
//! `vaultWrongPassword`: a wrong password and a tampered ciphertext look the
//! same to GCM, the overwhelmingly likely cause is the password, and the code
//! never echoes secrets.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::Aes256Gcm;
use argon2::{Algorithm, Argon2, Params, Version};
use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::Zeroizing;

use super::model::KdfParams;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
pub const SALT_LEN: usize = 16;

/// Cryptographically-secure random bytes from the OS CSPRNG.
pub fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    OsRng.fill_bytes(&mut buf);
    buf
}

/// Derive a 32-byte AES key from a password + the header's salt/params using
/// Argon2id. The returned key zeroizes itself when dropped.
///
/// The bounds check comes first because `params` may have come straight off
/// disk, where nothing authenticates it: the header sits outside the AES-GCM
/// ciphertext, so a tampered `mCost` reaches the allocator before any tag is
/// ever verified. See `KdfParams::validate`.
pub fn derive_key(
    password: &[u8],
    salt: &[u8],
    params: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, String> {
    params.validate()?;
    let cfg = Params::new(params.m_cost, params.t_cost, params.p_cost, Some(KEY_LEN))
        .map_err(|_| "vaultKdfInvalid".to_string())?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, cfg);

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password, salt, key.as_mut())
        .map_err(|_| "vaultKeyDerivationFailed".to_string())?;
    Ok(key)
}

/// Seal plaintext under the key, returning a fresh nonce and the ciphertext
/// (which embeds the GCM auth tag).
pub fn seal(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let cipher = Aes256Gcm::new(key.into());
    let nonce_bytes = random_bytes(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "vaultEncryptFailed".to_string())?;
    let ciphertext = cipher
        .encrypt(&nonce.into(), plaintext)
        .map_err(|_| "vaultEncryptFailed".to_string())?;
    Ok((nonce_bytes, ciphertext))
}

/// Open ciphertext under the key. Fails on a wrong key or any tampering.
pub fn open(key: &[u8; KEY_LEN], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    let nonce: [u8; NONCE_LEN] = nonce
        .try_into()
        .map_err(|_| "vaultFileCorrupt|nonce".to_string())?;
    let cipher = Aes256Gcm::new(key.into());
    cipher
        .decrypt(&nonce.into(), ciphertext)
        .map_err(|_| "vaultWrongPassword".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALT: &[u8] = b"sixteen_byte_slt";

    /// Known answer for the parameters this build writes. Every vault on disk
    /// was sealed with a key from this function, so a dependency upgrade
    /// that changes one output bit makes every existing vault unopenable.
    #[test]
    fn key_derivation_matches_the_known_answer() {
        let key = derive_key(b"correct horse", SALT, &KdfParams::default()).unwrap();
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "bd1bb14c874996956dc97c7b9f7b25ee38c9383e47ded8d8b84bfcbb6b61b780");
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex"))
            .collect()
    }

    /// Known answer for AES-256-GCM: a vault sealed by an older build must
    /// still open after an aes-gcm upgrade, and sealing must still produce
    /// the same bytes. The test-only key, nonce and ciphertext are read at
    /// run time from `tests/fixtures/aes_gcm_vault_vector.json`.
    #[test]
    fn aes_gcm_opens_the_known_ciphertext() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/aes_gcm_vault_vector.json");
        let raw = std::fs::read_to_string(&path).expect("fixture readable");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("fixture parses");
        let field = |k: &str| v[k].as_str().expect(k).to_string();
        let key: [u8; KEY_LEN] = unhex(&field("keyHex")).try_into().expect("key length");
        let nonce: [u8; NONCE_LEN] = unhex(&field("nonceHex")).try_into().expect("nonce length");
        let plain = field("plaintext").into_bytes();
        let ct = unhex(&field("ciphertextHex"));
        assert_eq!(open(&key, &nonce, &ct).expect("open"), plain);
        let cipher = Aes256Gcm::new((&key).into());
        let again = cipher.encrypt(&nonce.into(), plain.as_slice()).expect("seal");
        assert_eq!(again, ct);
    }

    #[test]
    fn the_parameters_the_app_itself_writes_are_accepted() {
        // The ceiling is worthless if it rejects vaults this build creates.
        assert!(derive_key(b"correct horse", SALT, &KdfParams::default()).is_ok());
    }

    #[test]
    fn a_header_asking_for_four_gibibytes_is_refused_before_argon2_allocates() {
        // mCost is in KiB, so 4_000_000 is a ~4 GiB allocation on unlock.
        let tampered = KdfParams {
            m_cost: 4_000_000,
            ..KdfParams::default()
        };
        assert_eq!(
            derive_key(b"correct horse", SALT, &tampered).unwrap_err(),
            KdfParams::OUT_OF_RANGE
        );
    }

    #[test]
    fn every_bound_is_enforced_on_both_sides() {
        let out_of_range = [
            KdfParams {
                m_cost: KdfParams::M_COST_MIN - 1,
                ..KdfParams::default()
            },
            KdfParams {
                m_cost: KdfParams::M_COST_MAX + 1,
                ..KdfParams::default()
            },
            KdfParams {
                t_cost: KdfParams::T_COST_MIN - 1,
                ..KdfParams::default()
            },
            KdfParams {
                t_cost: KdfParams::T_COST_MAX + 1,
                ..KdfParams::default()
            },
            KdfParams {
                p_cost: KdfParams::P_COST_MIN - 1,
                ..KdfParams::default()
            },
            KdfParams {
                p_cost: KdfParams::P_COST_MAX + 1,
                ..KdfParams::default()
            },
        ];
        for params in out_of_range {
            assert_eq!(params.validate().unwrap_err(), KdfParams::OUT_OF_RANGE);
        }
        // And the edges themselves stay usable, or the range is a lie.
        for params in [
            KdfParams {
                m_cost: KdfParams::M_COST_MIN,
                ..KdfParams::default()
            },
            KdfParams {
                m_cost: KdfParams::M_COST_MAX,
                ..KdfParams::default()
            },
            KdfParams {
                t_cost: KdfParams::T_COST_MIN,
                ..KdfParams::default()
            },
            KdfParams {
                t_cost: KdfParams::T_COST_MAX,
                ..KdfParams::default()
            },
            KdfParams {
                p_cost: KdfParams::P_COST_MIN,
                ..KdfParams::default()
            },
            KdfParams {
                p_cost: KdfParams::P_COST_MAX,
                ..KdfParams::default()
            },
        ] {
            assert!(params.validate().is_ok(), "{params:?} should be in range");
        }
    }

    #[test]
    fn a_bad_header_and_a_wrong_password_are_different_errors() {
        // The user has to be able to tell "your file is damaged" from "you
        // mistyped": one is a reset, the other is another attempt.
        let tampered = KdfParams {
            m_cost: u32::MAX,
            ..KdfParams::default()
        };
        let header_error = derive_key(b"pw", SALT, &tampered).unwrap_err();

        let right = derive_key(b"right", SALT, &KdfParams::default()).expect("derive");
        let wrong = derive_key(b"wrong", SALT, &KdfParams::default()).expect("derive");
        let (nonce, ct) = seal(&right, b"[]").expect("seal");
        let password_error = open(&wrong, &nonce, &ct).unwrap_err();

        assert_eq!(header_error, KdfParams::OUT_OF_RANGE);
        assert_eq!(password_error, "vaultWrongPassword");
        assert_ne!(header_error, password_error);
    }
}
