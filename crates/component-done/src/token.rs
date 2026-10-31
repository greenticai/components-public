//! `verify_token`: check an email action link (see
//! `docs/done/channels/email-link-format.md`).
//!
//! `token = b64url(payload_json) "." b64url(mac)`, no padding, with
//! `mac = HMAC-SHA256(done/link_key, "v1\n" d "\n" a "\n" r "\n" e "\n" n)`.
//!
//! This op is only the cryptographic half. It reports the nonce; the SINGLE USE
//! rule (D6) belongs to the flow, which looks the nonce up in the SoR and appends
//! it only after the transition succeeds, so a refused transition never burns a
//! link. Nothing here panics: every malformed input is a value.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use crate::env::Env;
use crate::mac;
use crate::ops::{err, str_field};
use crate::uuid7;

/// Clock skew allowed past `e` before a link counts as expired.
pub const SKEW_S: u64 = 60;
/// Upper bound on a token's length; a real one is well under 400 bytes.
pub const MAX_TOKEN_LEN: usize = 2048;
/// The actions a link may carry: the recipient's four (spec 3.3 names) and the
/// two answers of a permission prompt.
pub const ACTIONS: &[&str] = &[
    "accept",
    "done",
    "more_time",
    "cant_do",
    "allow_sender",
    "deny_sender",
];
/// Default life of a link: 72 hours (email-link-format.md).
pub const DEFAULT_TTL_S: u64 = 72 * 3600;

#[derive(Debug, PartialEq, Eq)]
pub struct Link {
    pub done_id: String,
    pub action: String,
    pub recipient: String,
    pub expires_at: u64,
    pub nonce: String,
}

/// A failure with the stable code a flow branches on.
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: &'static str,
}

const fn refuse(code: &'static str, message: &'static str) -> Refusal {
    Refusal { code, message }
}

/// A field that is part of the signed message must not contain the separator.
fn field(payload: &Value, key: &str) -> Option<String> {
    let s = payload.get(key)?.as_str()?;
    (!s.is_empty() && !s.contains('\n')).then(|| s.to_string())
}

fn parse(token: &str) -> Result<(Link, Vec<u8>), Refusal> {
    let bad = || refuse("bad_token", "the link is not a valid Done link");
    if token.len() > MAX_TOKEN_LEN {
        return Err(bad());
    }
    let (p, m) = token.split_once('.').ok_or_else(bad)?;
    if m.contains('.') {
        return Err(bad());
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(p).map_err(|_| bad())?;
    let mac = URL_SAFE_NO_PAD.decode(m).map_err(|_| bad())?;
    let payload: Value = serde_json::from_slice(&payload_bytes).map_err(|_| bad())?;
    if payload.get("v").and_then(Value::as_u64) != Some(1) {
        return Err(bad());
    }
    let done_id = field(&payload, "d").ok_or_else(bad)?;
    let action = field(&payload, "a").ok_or_else(bad)?;
    let recipient = field(&payload, "r").ok_or_else(bad)?;
    let nonce = field(&payload, "n").ok_or_else(bad)?;
    let expires_at = payload.get("e").and_then(Value::as_u64).ok_or_else(bad)?;
    if !uuid7::is_well_formed(&done_id) || !ACTIONS.contains(&action.as_str()) {
        return Err(bad());
    }
    let link = Link {
        done_id,
        action,
        recipient,
        expires_at,
        nonce,
    };
    Ok((link, mac))
}

/// The bytes the MAC covers.
fn message(l: &Link) -> String {
    format!(
        "v1\n{}\n{}\n{}\n{}\n{}",
        l.done_id, l.action, l.recipient, l.expires_at, l.nonce
    )
}

/// Mint a link token. `None` when a field cannot be signed (empty, contains the
/// separator, unknown action), so a caller never emits a token `check` refuses.
pub fn issue(
    key: &[u8],
    done_id: &str,
    action: &str,
    recipient: &str,
    expires_at: u64,
    nonce: &str,
) -> Option<String> {
    let link = Link {
        done_id: done_id.to_string(),
        action: action.to_string(),
        recipient: recipient.to_string(),
        expires_at,
        nonce: nonce.to_string(),
    };
    let ok = |s: &str| !s.is_empty() && !s.contains('\n');
    if !ok(done_id)
        || !ok(recipient)
        || !ok(nonce)
        || !uuid7::is_well_formed(done_id)
        || !ACTIONS.contains(&action)
    {
        return None;
    }
    let payload = json!({
        "v": 1, "d": link.done_id, "a": link.action, "r": link.recipient,
        "e": link.expires_at, "n": link.nonce,
    });
    let mac = mac::hmac_sha256(key, message(&link).as_bytes());
    Some(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload.to_string()),
        URL_SAFE_NO_PAD.encode(mac)
    ))
}

/// Parse, authenticate, then check expiry. The MAC is checked BEFORE the expiry
/// so a forged token cannot learn anything from which refusal it gets.
pub fn check(key: &[u8], token: &str, now_s: u64) -> Result<Link, Refusal> {
    let (link, mac) = parse(token)?;
    let expected = mac::hmac_sha256(key, message(&link).as_bytes());
    if !mac::ct_eq(&expected, &mac) {
        return Err(refuse(
            "bad_signature",
            "the link's signature does not match",
        ));
    }
    if now_s > link.expires_at.saturating_add(SKEW_S) {
        return Err(refuse("link_expired", "this link has expired"));
    }
    Ok(link)
}

pub fn verify_token(env: &dyn Env, input: &Value) -> Value {
    let Some(token) = str_field(input, "token") else {
        return err("invalid_request", "`token` is required");
    };
    let Some(raw_key) = str_field(input, "link_key") else {
        return err(
            "unauthorised",
            "missing `link_key` (wire it to the secret done/link_key)",
        );
    };
    let key = match mac::secret_by_name(env, raw_key) {
        Ok(k) => k,
        Err(e) => return err("unauthorised", e),
    };
    match check(key.as_bytes(), token, env.now_ms() / 1000) {
        Ok(l) => json!({
            "ok": true,
            "done_id": l.done_id,
            "action": l.action,
            "recipient": l.recipient,
            "expires_at": l.expires_at,
            "nonce": l.nonce,
        }),
        Err(r) => err(r.code, r.message),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::env::{HttpReq, Response};

    pub const ID: &str = "01890a5d-ac96-774b-bcce-b302099a8057";
    const KEY: &str = "link-key-123";

    pub struct Clock {
        pub secrets: Vec<(&'static str, &'static str)>,
        pub now_s: u64,
    }
    impl Env for Clock {
        fn secret(&self, name: &str) -> Result<Option<String>, String> {
            Ok(self
                .secrets
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_string()))
        }
        fn send(&self, _: HttpReq) -> Result<Response, String> {
            Err("no network".into())
        }
        fn now_ms(&self) -> u64 {
            self.now_s * 1000
        }
        fn random(&self) -> [u8; 10] {
            [0; 10]
        }
    }

    fn mint_with(key: &str, payload: &Value, signed: &Link) -> String {
        let mac = mac::hmac_sha256(key.as_bytes(), message(signed).as_bytes());
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(payload.to_string()),
            URL_SAFE_NO_PAD.encode(mac)
        )
    }

    fn link(action: &str, e: u64) -> Link {
        Link {
            done_id: ID.into(),
            action: action.into(),
            recipient: "person-1".into(),
            expires_at: e,
            nonce: "bm9uY2Ux".into(),
        }
    }

    pub fn mint(key: &str, action: &str, e: u64) -> String {
        let l = link(action, e);
        let payload = json!({"v":1,"d":l.done_id,"a":l.action,"r":l.recipient,"e":e,"n":l.nonce});
        mint_with(key, &payload, &l)
    }

    fn env(now_s: u64) -> Clock {
        Clock {
            secrets: vec![("done/link_key", KEY)],
            now_s,
        }
    }

    fn input(token: &str) -> Value {
        json!({ "token": token, "link_key": "secret:done/link_key" })
    }

    #[test]
    fn a_valid_link_reports_its_fields_and_the_nonce() {
        let out = verify_token(&env(1_000), &input(&mint(KEY, "accept", 5_000)));
        assert_eq!(
            out,
            json!({"ok":true,"done_id":ID,"action":"accept","recipient":"person-1",
                   "expires_at":5000,"nonce":"bm9uY2Ux"})
        );
    }

    #[test]
    fn expiry_allows_sixty_seconds_of_skew_and_no_more() {
        let t = mint(KEY, "done", 5_000);
        assert_eq!(verify_token(&env(5_060), &input(&t))["ok"], true);
        let out = verify_token(&env(5_061), &input(&t));
        assert_eq!(out["error"]["code"], "link_expired");
    }

    #[test]
    fn a_wrong_key_or_tampered_field_is_a_bad_signature() {
        let t = mint("other-key", "accept", 5_000);
        assert_eq!(
            verify_token(&env(1_000), &input(&t))["error"]["code"],
            "bad_signature"
        );
        // Same signature, different action in the payload.
        let genuine = mint(KEY, "accept", 5_000);
        let (_, mac) = genuine.split_once('.').unwrap();
        let l = link("done", 5_000);
        let payload = json!({"v":1,"d":l.done_id,"a":"done","r":l.recipient,"e":5000,"n":l.nonce});
        let forged = format!("{}.{mac}", URL_SAFE_NO_PAD.encode(payload.to_string()));
        assert_eq!(
            verify_token(&env(1_000), &input(&forged))["error"]["code"],
            "bad_signature"
        );
    }

    #[test]
    fn a_forged_expired_token_gets_bad_signature_not_expired() {
        let t = mint("other-key", "accept", 10);
        assert_eq!(
            verify_token(&env(9_999), &input(&t))["error"]["code"],
            "bad_signature"
        );
    }

    #[test]
    fn malformed_tokens_are_values_never_panics() {
        let l = link("accept", 5_000);
        let no_exp = json!({"v":1,"d":ID,"a":"accept","r":"p","n":"x"});
        let v2 = json!({"v":2,"d":ID,"a":"accept","r":"p","e":5000,"n":"x"});
        let bad_action = json!({"v":1,"d":ID,"a":"delete","r":"p","e":5000,"n":"x"});
        let newline = json!({"v":1,"d":ID,"a":"accept","r":"p\nq","e":5000,"n":"x"});
        let signed = |p: &Value| mint_with(KEY, p, &l);
        let cases = [
            "no-dot".to_string(),
            "a.b.c".to_string(),
            "!!!.???".to_string(),
            format!("{}.", URL_SAFE_NO_PAD.encode("{}")),
            format!("{}.AAAA", URL_SAFE_NO_PAD.encode("not json")),
            signed(&no_exp),
            signed(&v2),
            signed(&bad_action),
            signed(&newline),
            "x".repeat(MAX_TOKEN_LEN + 1),
            format!("{}.==", URL_SAFE_NO_PAD.encode("{}")),
        ];
        for t in cases {
            let out = verify_token(&env(1_000), &input(&t));
            assert_eq!(out["ok"], false, "{t}");
            assert_eq!(out["error"]["code"], "bad_token", "{t}");
        }
    }

    #[test]
    fn the_key_must_be_a_secret_reference_that_is_set() {
        let t = mint(KEY, "accept", 5_000);
        let literal = json!({ "token": t, "link_key": KEY });
        let out = verify_token(&env(1_000), &literal);
        assert_eq!(out["error"]["code"], "unauthorised");
        assert!(!out.to_string().contains(KEY));

        let mut unset = env(1_000);
        unset.secrets.clear();
        let out = verify_token(&unset, &input(&t));
        assert_eq!(out["error"]["code"], "unauthorised");
        assert!(
            out["error"]["message"]
                .as_str()
                .unwrap()
                .contains("done/link_key")
        );

        assert_eq!(
            verify_token(&env(1_000), &json!({}))["error"]["code"],
            "invalid_request"
        );
        assert_eq!(
            verify_token(&env(1_000), &json!({ "token": t }))["error"]["code"],
            "unauthorised"
        );
    }
}
