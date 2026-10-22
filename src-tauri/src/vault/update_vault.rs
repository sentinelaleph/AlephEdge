//! Update-persistence proof, vault part. The OS keychain is never touched:
//! the salt lives in an in-memory keychain and is carried in the fixture.
//! All values are test-only (no real key, no real password): the generator
//! makes a random password per fixture and writes it into the fixture's
//! `vault_test.json`, where the reader takes it from.
//!
//! Generator (on an OLDER release tree, with the store fixture's dir):
//!   AE_UPDATE_FIXTURE_OUT=<dir> cargo test --lib generate_update_vault -- --ignored
//! Reader (this build): `a_vault_sealed_by_an_older_release_opens` reads every
//! fixture under store/fixtures/update/.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::model::{CredentialPermission, ExchangeCredential};
use super::{VaultManager, KEYCHAIN_SALT_USER};
use crate::app::keychain::memory::MemoryKeychain;
use crate::app::keychain::SecretStore;

const SERVICE: &str = "com.sentinelaleph.edge.updtest";

/// A random password for one generated fixture.
fn fixture_password() -> String {
    format!("test-only-{:032x}", rand::random::<u128>())
}

fn fake_cred(label: &str) -> ExchangeCredential {
    ExchangeCredential {
        exchange_id: if label == "main" { "binance" } else { "bybit" }.to_string(),
        label: label.to_string(),
        api_key: format!("TEST-ONLY-not-a-key-{label}"),
        api_secret: format!("TEST-ONLY-not-a-secret-{label}"),
        passphrase: None,
        permission: CredentialPermission::TradeOnly,
        added_at: 1_792_000_000_000,
        updated_at: None,
        key_type: Default::default(),
        exchange_created_at: Some(1_791_000_000_000),
        ip_restricted: Some(true),
    }
}

#[test]
#[ignore]
fn generate_update_vault() {
    let out = PathBuf::from(std::env::var("AE_UPDATE_FIXTURE_OUT").expect("AE_UPDATE_FIXTURE_OUT"));
    crate::store::update_common::assert_scratch_dir(&out);
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join("vault.edge");
    assert!(!path.exists());
    let ks = MemoryKeychain::default();
    let m = VaultManager::new();
    let password = fixture_password();
    m.create_in(&ks, SERVICE, &path, &password).expect("create");
    m.add(&path, fake_cred("main")).expect("add main");
    m.add(&path, fake_cred("second")).expect("add second");
    let salt = ks.get(SERVICE, KEYCHAIN_SALT_USER).expect("salt");
    let creds: Vec<Value> = ["main", "second"].iter().map(|l| json!(fake_cred(l))).collect();
    let body = json!({"service": SERVICE, "saltB64": salt, "password": password, "creds": creds, "version": env!("CARGO_PKG_VERSION")});
    std::fs::write(out.join("vault_test.json"), serde_json::to_string_pretty(&body).unwrap()).unwrap();
}

/// Opens `vault.edge` in `dir` with this build; returns the credentials.
pub(crate) fn open_with_this_build(dir: &Path, vault_json: &Value) -> Result<(VaultManager, MemoryKeychain), String> {
    let ks = MemoryKeychain::default();
    ks.set(SERVICE, KEYCHAIN_SALT_USER, vault_json["saltB64"].as_str().unwrap()).unwrap();
    let m = VaultManager::new();
    m.unlock_in(&ks, SERVICE, &dir.join("vault.edge"), vault_json["password"].as_str().unwrap())?;
    Ok((m, ks))
}

fn fixture_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/fixtures/update");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("vault.edge").exists()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn a_vault_sealed_by_an_older_release_opens() {
    let dirs = fixture_dirs();
    assert!(!dirs.is_empty(), "no update fixtures found");
    for fx in dirs {
        let tmp = std::env::temp_dir().join(format!("ae-upd-vault-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::copy(fx.join("vault.edge"), tmp.join("vault.edge")).unwrap();
        let vj: Value = serde_json::from_str(&std::fs::read_to_string(fx.join("vault_test.json")).unwrap()).unwrap();
        let (m, ks) = open_with_this_build(&tmp, &vj).unwrap_or_else(|e| panic!("{}: {e}", fx.display()));
        let creds = m.credentials_for_test();
        assert_eq!(json!(creds), vj["creds"], "{}: credentials changed", fx.display());
        // A wrong password (the fixture's, changed) is still refused.
        let wrong = format!("{}-not", vj["password"].as_str().unwrap());
        let m2 = VaultManager::new();
        assert_eq!(
            m2.unlock_in(&ks, SERVICE, &tmp.join("vault.edge"), &wrong).unwrap_err(),
            "vaultWrongPassword"
        );
        // This build re-seals it (a label change); an older build must open
        // that file too (downgrade, see update_persist_tests).
        m.rename(&tmp.join("vault.edge"), "bybit", "second", "renamed").expect("rename");
        if let Ok(dg) = std::env::var("AE_UPDATE_DOWNGRADE_OUT") {
            let name = fx.file_name().unwrap().to_string_lossy().to_string();
            let d = PathBuf::from(dg).join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::copy(tmp.join("vault.edge"), d.join("vault.edge")).unwrap();
            std::fs::copy(fx.join("vault_test.json"), d.join("vault_test.json")).unwrap();
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

/// Downgrade: an OLDER build opens a vault this (newer) build re-sealed.
/// Run on the older tree: AE_UPDATE_DOWNGRADE_IN=<dir> cargo test --lib older_build_opens_newer_vault -- --ignored
#[test]
#[ignore]
fn older_build_opens_newer_vault() {
    let root = PathBuf::from(std::env::var("AE_UPDATE_DOWNGRADE_IN").expect("AE_UPDATE_DOWNGRADE_IN"));
    let mut n = 0;
    for e in std::fs::read_dir(&root).unwrap() {
        let d = e.unwrap().path();
        if !d.join("vault.edge").exists() {
            continue;
        }
        let vj: Value = serde_json::from_str(&std::fs::read_to_string(d.join("vault_test.json")).unwrap()).unwrap();
        let (m, _) = open_with_this_build(&d, &vj).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
        let labels: Vec<String> = m.credentials_for_test().iter().map(|c| c.label.clone()).collect();
        assert_eq!(labels, vec!["main".to_string(), "renamed".to_string()], "{}", d.display());
        n += 1;
    }
    assert!(n > 0, "no vault in {}", root.display());
}

impl VaultManager {
    fn credentials_for_test(&self) -> Vec<ExchangeCredential> {
        self.use_session().as_ref().expect("unlocked").creds.clone()
    }
}
