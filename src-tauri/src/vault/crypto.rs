//! Vault cryptographic primitives: Argon2id key derivation and AES-256-GCM
//! authenticated encryption. This module holds no state and never logs inputs.
//!
//! Errors are stable codes the UI localizes (`errors.*`). A failed AEAD open is
//! `vaultWrongPassword`: a wrong password and a tampered ciphertext look the
//! same to GCM, the overwhelmingly likely cause is the password, and the code
//! never echoes secrets.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
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
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce_bytes = random_bytes(NONCE_LEN);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| "vaultEncryptFailed".to_string())?;
    Ok((nonce_bytes, ciphertext))
}

/// Open ciphertext under the key. Fails on a wrong key or any tampering.
pub fn open(key: &[u8; KEY_LEN], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    if nonce.len() != NONCE_LEN {
        return Err("vaultFileCorrupt|nonce".to_string());
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| "vaultWrongPassword".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALT: &[u8] = b"sixteen_byte_slt";

    #[test]
    fn the_parameters_the_app_itself_writes_are_accepted() {
        // The ceiling is worthless if it rejects vaults this build creates.
        assert!(derive_key(b"correct horse", SALT, &KdfParams::default()).is_ok());
    }

    #[test]
    fn a_header_asking_for_four_gibibytes_is_refused_before_argon2_allocates() {
        // The exact value from the pre-publication review: mCost is in KiB, so
        // 4_000_000 is a ~4 GiB allocation on unlock.
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
