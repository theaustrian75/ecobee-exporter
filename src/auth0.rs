//! Auth0 + PKCE client for ecobee's mobile-app OAuth tenant.
//!
//! Constants here match the ecobee Android app's Auth0 Universal Login
//! flow (`auth.ecobee.com`). They are *public*
//! client parameters baked into the app binary — there is no
//! `client_secret` because the client is a public PKCE client.
//!
//! Two flows are implemented:
//!
//!   - **Authorization code + PKCE** for the one-time interactive
//!     bootstrap (`ecobee-login` binary). Driven by `build_authorize_url`
//!     and `exchange_code`.
//!   - **Refresh token** for the long-running exporter. `refresh_token`.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

pub const AUTH_DOMAIN: &str = "auth.ecobee.com";
pub const CLIENT_ID: &str = "yg66ag34vWdf2Hs4oO2ih2BvI16KrkOR";
pub const REDIRECT_URI: &str = "https://auth.ecobee.com/android/com.ecobee.athenamobile/callback";
pub const AUDIENCE: &str = "https://prod.ecobee.com/api/v1";
pub const SCOPE: &str = "openid offline_access smartWrite piiWrite piiRead";

const TOKEN_PATH: &str = "/oauth/token";
const AUTHORIZE_PATH: &str = "/authorize";

#[derive(Debug, Error)]
pub enum Auth0Error {
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("url: {0}")]
    Url(#[from] url::ParseError),
    #[error("token endpoint returned {status}: {body}")]
    TokenError { status: u16, body: String },
    #[error("response missing required field `{0}`")]
    MissingField(&'static str),
    #[error("rng: {0}")]
    Rng(String),
}

/// A freshly-minted PKCE pair, plus the matching `state` and `nonce` we'll
/// send on the authorize request. All four are needed to complete the
/// flow: the `verifier` to exchange the code, and `state` to verify the
/// callback URL was minted by us.
#[derive(Debug, Clone)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
    pub nonce: String,
}

impl PkcePair {
    pub fn generate() -> Result<Self, Auth0Error> {
        let verifier = random_url_safe(32)?;
        let challenge = challenge_from_verifier(&verifier);
        let state = random_url_safe(24)?;
        let nonce = random_url_safe(24)?;
        Ok(Self {
            verifier,
            challenge,
            state,
            nonce,
        })
    }
}

fn random_url_safe(byte_len: usize) -> Result<String, Auth0Error> {
    let mut buf = vec![0u8; byte_len];
    rand::rng()
        .try_fill_bytes(&mut buf)
        .map_err(|e| Auth0Error::Rng(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(&buf))
}

fn challenge_from_verifier(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// Construct the `/authorize` URL the user should open in their browser.
/// Matches the parameters the ecobee Android app uses, with `prompt=login`
/// so the user always gets a fresh login page.
pub fn build_authorize_url(pkce: &PkcePair) -> Result<Url, Auth0Error> {
    let mut url = Url::parse(&format!("https://{AUTH_DOMAIN}{AUTHORIZE_PATH}"))?;
    url.query_pairs_mut()
        .append_pair("scope", SCOPE)
        .append_pair("prompt", "login")
        .append_pair("audience", AUDIENCE)
        .append_pair("response_type", "code")
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("state", &pkce.state)
        .append_pair("nonce", &pkce.nonce);
    Ok(url)
}

/// Auth0 `/oauth/token` response. We deliberately keep only what we use;
/// extra fields like `id_token` and `scope` are accepted via `flatten` so
/// nothing breaks if Auth0 starts returning them differently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    pub token_type: String,
    pub expires_in: i64,
}

#[derive(Serialize)]
struct CodeExchangeBody<'a> {
    grant_type: &'static str,
    client_id: &'static str,
    code: &'a str,
    redirect_uri: &'static str,
    code_verifier: &'a str,
}

#[derive(Serialize)]
struct RefreshBody<'a> {
    grant_type: &'static str,
    client_id: &'static str,
    refresh_token: &'a str,
}

pub async fn exchange_code(
    http: &reqwest::Client,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse, Auth0Error> {
    post_token(
        http,
        &CodeExchangeBody {
            grant_type: "authorization_code",
            client_id: CLIENT_ID,
            code,
            redirect_uri: REDIRECT_URI,
            code_verifier: verifier,
        },
    )
    .await
}

pub async fn refresh_token(
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<TokenResponse, Auth0Error> {
    post_token(
        http,
        &RefreshBody {
            grant_type: "refresh_token",
            client_id: CLIENT_ID,
            refresh_token,
        },
    )
    .await
}

async fn post_token<B: Serialize>(
    http: &reqwest::Client,
    body: &B,
) -> Result<TokenResponse, Auth0Error> {
    let url = format!("https://{AUTH_DOMAIN}{TOKEN_PATH}");
    let resp = http.post(&url).form(body).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(Auth0Error::TokenError {
            status: status.as_u16(),
            body: text,
        });
    }
    let tokens: TokenResponse = resp.json().await?;
    if tokens.access_token.is_empty() {
        return Err(Auth0Error::MissingField("access_token"));
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636 Appendix B test vector for the S256 code challenge.
    #[test]
    fn rfc7636_s256_test_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = challenge_from_verifier(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn generated_verifier_is_43_chars_url_safe() {
        let pair = PkcePair::generate().unwrap();
        assert_eq!(pair.verifier.len(), 43);
        for c in pair.verifier.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '-' || c == '_',
                "non-url-safe char in verifier: {c}"
            );
        }
        assert_eq!(pair.challenge, challenge_from_verifier(&pair.verifier));
    }

    #[test]
    fn authorize_url_has_required_oauth_params() {
        let pair = PkcePair {
            verifier: "v".into(),
            challenge: "c".into(),
            state: "s".into(),
            nonce: "n".into(),
        };
        let url = build_authorize_url(&pair).unwrap();
        let qs: std::collections::HashMap<_, _> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(qs.get("client_id").map(String::as_str), Some(CLIENT_ID));
        assert_eq!(
            qs.get("redirect_uri").map(String::as_str),
            Some(REDIRECT_URI)
        );
        assert_eq!(qs.get("audience").map(String::as_str), Some(AUDIENCE));
        assert_eq!(qs.get("scope").map(String::as_str), Some(SCOPE));
        assert_eq!(qs.get("response_type").map(String::as_str), Some("code"));
        assert_eq!(
            qs.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(qs.get("code_challenge").map(String::as_str), Some("c"));
        assert_eq!(qs.get("state").map(String::as_str), Some("s"));
        assert_eq!(qs.get("nonce").map(String::as_str), Some("n"));
    }

    #[test]
    fn token_response_parses_real_shape() {
        let json = r#"{
            "access_token": "eyJ.aaa.bbb",
            "refresh_token": "v1.rt.zzz",
            "id_token": "eyJ.iii.jjj",
            "scope": "openid offline_access smartWrite piiWrite piiRead",
            "expires_in": 86400,
            "token_type": "Bearer"
        }"#;
        let tok: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(tok.access_token, "eyJ.aaa.bbb");
        assert_eq!(tok.refresh_token.as_deref(), Some("v1.rt.zzz"));
        assert_eq!(tok.expires_in, 86400);
        assert_eq!(tok.token_type, "Bearer");
    }

    #[test]
    fn token_response_without_refresh_is_ok() {
        let json = r#"{
            "access_token": "x",
            "token_type": "Bearer",
            "expires_in": 3600
        }"#;
        let tok: TokenResponse = serde_json::from_str(json).unwrap();
        assert!(tok.refresh_token.is_none());
    }
}
