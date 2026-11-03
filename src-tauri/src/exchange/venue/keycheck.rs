//! What a key may do, read from each venue's own answer. Pure parsers, one
//! per venue, so every rule is unit-tested on recorded answer shapes.
//!
//! Every parser fails CLOSED: an answer it cannot read returns None, and the
//! vault then refuses the key with a retryable reason. A permission the
//! venue does not name at all is reported as such in the README matrix.
//!
//! Sources (22 Oct 2026, official API docs):
//! - Bitget  GET /api/v2/spot/account/info: `authorities` codes, `coow` +
//!   `cpow` futures order/position write, `wwow` withdraw, `wtow` transfer.
//! - Gate    GET /api/v4/account/main_keys: per key `perms` [{name,
//!   read_only}]. The answer shape is not fully documented; the parser only
//!   accepts a row for this key and refuses any writable `wallet` or
//!   `withdraw*` perm.
//! - BingX   GET /openApi/v1/account/apiPermissions: `enableFutures`,
//!   `enableWithdrawals`, `enableInternalTransfer`,
//!   `permitsUniversalTransfer` (Binance's shape).
//! - WOO X   GET /v3/account/credentials (ccxt `v3_private_get_account_credentials`):
//!   the row of this key, its `permission(s)` text; any "withdraw" or
//!   "transfer" is refused. Shape not confirmed on a live account.
//! - Deribit public/auth (client_signature): `scope`, `trade:read_write`,
//!   `wallet:read_write` (withdraw and transfer).

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use ccxt::Value;

/// What a key may do, as the venue reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyPerms {
    /// It can place and close perpetual orders.
    pub trade: bool,
    pub withdraw: bool,
    /// It can move funds to another account or sub-account.
    pub transfer: bool,
}

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    match v {
        Value::Dict(d) => d.get(key).unwrap_or(&Value::Null),
        _ => &Value::Null,
    }
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.to_string()),
        Value::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

fn truthy(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Int(i) => Some(*i != 0),
        Value::Str(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn arr(v: &Value) -> Option<&[Value]> {
    match v {
        Value::Arr(a) => Some(a.as_slice()),
        _ => None,
    }
}

pub fn bitget(r: &Value) -> Option<KeyPerms> {
    let codes: Vec<String> = arr(get(get(r, "data"), "authorities"))?.iter().filter_map(text).collect();
    let has = |c: &str| codes.iter().any(|x| x == c);
    Some(KeyPerms { trade: has("coow") && has("cpow"), withdraw: has("wwow"), transfer: has("wtow") })
}

pub fn gate(r: &Value, api_key: &str) -> Option<KeyPerms> {
    // A list of keys (top level or under `data`); only this key's row counts.
    let rows = arr(r).or_else(|| arr(get(r, "data")))?;
    let row = rows.iter().find(|row| ["key", "api_key", "apiKey"].iter().any(|k| text(get(row, k)).as_deref() == Some(api_key)))?;
    let perms = arr(get(row, "perms"))?;
    let writable = |pred: &dyn Fn(&str) -> bool| {
        perms.iter().any(|p| text(get(p, "name")).is_some_and(|n| pred(&n.to_ascii_lowercase())) && truthy(get(p, "read_only")) == Some(false))
    };
    Some(KeyPerms {
        trade: writable(&|n| n == "futures"),
        withdraw: writable(&|n| n.contains("withdraw")),
        transfer: writable(&|n| n == "wallet"),
    })
}

pub fn bingx(r: &Value) -> Option<KeyPerms> {
    let d = get(r, "data");
    Some(KeyPerms {
        trade: truthy(get(d, "enableFutures"))?,
        withdraw: truthy(get(d, "enableWithdrawals"))?,
        transfer: truthy(get(d, "enableInternalTransfer")).unwrap_or(false) || truthy(get(d, "permitsUniversalTransfer")).unwrap_or(false),
    })
}

pub fn woo(r: &Value, api_key: &str) -> Option<KeyPerms> {
    let d = get(r, "data");
    let rows = arr(get(d, "rows")).or_else(|| arr(d)).or_else(|| arr(get(r, "rows")))?;
    let row = rows.iter().find(|row| ["apiKey", "api_key", "key"].iter().any(|k| text(get(row, k)).as_deref() == Some(api_key)))?;
    let perm = match get(row, "permission") {
        Value::Null => get(row, "permissions"),
        v => v,
    };
    let joined = match perm {
        Value::Arr(a) => a.iter().filter_map(text).collect::<Vec<_>>().join(","),
        v => text(v)?,
    }
    .to_ascii_lowercase();
    Some(KeyPerms { trade: joined.contains("trade"), withdraw: joined.contains("withdraw"), transfer: joined.contains("transfer") })
}

pub fn deribit(r: &Value) -> Option<KeyPerms> {
    let scope = text(get(get(r, "result"), "scope"))?;
    let has = |x: &str| scope.split_whitespace().any(|s| s == x);
    let wallet = has("wallet:read_write") || has("wallets:read_write");
    Some(KeyPerms { trade: has("trade:read_write"), withdraw: wallet, transfer: wallet })
}

/// Deribit `public/auth` with `grant_type=client_signature`: the secret
/// never leaves the device, only an HMAC of (timestamp, nonce, data).
pub fn deribit_auth_params(client_id: &str, secret: &str, timestamp_ms: u64, nonce: &str) -> [(String, String); 6] {
    let data = "";
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts a key of any length");
    mac.update(format!("{timestamp_ms}\n{nonce}\n{data}").as_bytes());
    let sig: String = mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect();
    [
        ("grant_type".into(), "client_signature".into()),
        ("client_id".into(), client_id.into()),
        ("timestamp".into(), timestamp_ms.to_string()),
        ("nonce".into(), nonce.into()),
        ("data".into(), data.into()),
        ("signature".into(), sig),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(j: serde_json::Value) -> Value {
        Value::from(j)
    }

    #[test]
    fn bitget_reads_authority_codes() {
        let r = |codes: &[&str]| v(serde_json::json!({"code": "00000", "data": {"userId": "1", "authorities": codes}}));
        assert_eq!(bitget(&r(&["coor", "cpor", "coow", "cpow"])), Some(KeyPerms { trade: true, withdraw: false, transfer: false }));
        assert_eq!(bitget(&r(&["coor", "cpor"])).unwrap().trade, false, "read only");
        assert!(bitget(&r(&["coow", "cpow", "wwow"])).unwrap().withdraw);
        assert!(bitget(&r(&["coow", "cpow", "wtow"])).unwrap().transfer);
        assert_eq!(bitget(&v(serde_json::json!({"code": "00000", "data": {}}))), None, "no list: refused");
    }

    #[test]
    fn gate_reads_only_this_keys_row() {
        let r = v(serde_json::json!([
            {"key": "other", "perms": [{"name": "futures", "read_only": false}, {"name": "wallet", "read_only": false}]},
            {"key": "mine", "perms": [{"name": "futures", "read_only": false}, {"name": "wallet", "read_only": true}]}
        ]));
        assert_eq!(gate(&r, "mine"), Some(KeyPerms { trade: true, withdraw: false, transfer: false }));
        assert_eq!(gate(&r, "other").unwrap().transfer, true);
        assert_eq!(gate(&r, "absent"), None);
        let wd = v(serde_json::json!({"data": [{"key": "k", "perms": [{"name": "futures", "read_only": false}, {"name": "withdraw", "read_only": false}]}]}));
        assert!(gate(&wd, "k").unwrap().withdraw);
    }

    #[test]
    fn bingx_reads_binance_style_flags() {
        let r = |f: bool, w: bool, t: bool| {
            v(serde_json::json!({"code": 0, "data": {"enableReading": true, "enableFutures": f, "enableWithdrawals": w, "enableInternalTransfer": t, "permitsUniversalTransfer": false}}))
        };
        assert_eq!(bingx(&r(true, false, false)), Some(KeyPerms { trade: true, withdraw: false, transfer: false }));
        assert!(bingx(&r(true, true, false)).unwrap().withdraw);
        assert!(bingx(&r(true, false, true)).unwrap().transfer);
        assert_eq!(bingx(&v(serde_json::json!({"code": 0, "data": {"enableFutures": true}}))), None, "withdraw flag missing");
    }

    #[test]
    fn woo_reads_this_keys_permission_text_and_fails_closed() {
        let r = v(serde_json::json!({"success": true, "data": {"rows": [
            {"apiKey": "k1", "permission": "Read,Enable trade,"},
            {"apiKey": "k2", "permission": "Read,Enable trade,Enable withdrawal"}
        ]}}));
        assert_eq!(woo(&r, "k1"), Some(KeyPerms { trade: true, withdraw: false, transfer: false }));
        assert!(woo(&r, "k2").unwrap().withdraw);
        assert_eq!(woo(&r, "k3"), None);
        assert_eq!(woo(&v(serde_json::json!({"success": true})), "k1"), None);
    }

    #[test]
    fn deribit_reads_the_granted_scope() {
        let r = |s: &str| v(serde_json::json!({"jsonrpc": "2.0", "result": {"access_token": "x", "scope": s}}));
        assert_eq!(deribit(&r("account:read trade:read_write wallet:read")), Some(KeyPerms { trade: true, withdraw: false, transfer: false }));
        let wd = deribit(&r("trade:read_write wallet:read_write")).unwrap();
        assert!(wd.withdraw && wd.transfer);
        assert!(!deribit(&r("trade:read")).unwrap().trade);
    }

    #[test]
    fn deribit_signature_never_carries_the_secret() {
        let p = deribit_auth_params("cid", "s3cret", 1_700_000_000_000, "abc");
        assert!(p.iter().all(|(_, v)| !v.contains("s3cret")));
        let sig = &p.iter().find(|(k, _)| k == "signature").unwrap().1;
        assert_eq!(sig.len(), 64);
        assert_eq!(p, deribit_auth_params("cid", "s3cret", 1_700_000_000_000, "abc"), "deterministic");
    }
}
