//! OpenID Connect over HTTPS (ADR-0025): discovery, the authorization URL, the code
//! exchange (PKCE), and ID token signature verification with `ring`.
//!
//! Only `RS256` and `ES256` are accepted; `none`, HMAC and every other algorithm are
//! refused, and the key must come from the provider's JWKS (no keys embedded in the
//! token). Discovery documents and keys are cached for an hour and the keys are fetched
//! again once when a token names an unknown key ID (key rotation).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::domain::{IdClaims, OidcClient, SsoProvider};

const CACHE_FOR: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}

/// One key of a JWKS.
#[derive(Debug, Clone, Deserialize)]
pub struct Jwk {
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default, rename = "use")]
    usage: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
    #[serde(default)]
    crv: Option<String>,
    #[serde(default)]
    x: Option<String>,
    #[serde(default)]
    y: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

struct Cached {
    at: Instant,
    discovery: Discovery,
    keys: Vec<Jwk>,
}

/// The production [`OidcClient`].
pub struct HttpOidcClient {
    http: reqwest::Client,
    cache: RwLock<HashMap<String, Cached>>,
}

impl HttpOidcClient {
    /// A client with 15-second timeouts and the Mozilla root certificates (ring).
    ///
    /// # Errors
    ///
    /// The TLS configuration cannot be built.
    pub fn new() -> Result<Self, String> {
        let roots = tokio_rustls::rustls::RootCertStore::from_iter(
            webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
        );
        let tls = tokio_rustls::rustls::ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("Cooper/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            cache: RwLock::new(HashMap::new()),
        })
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("{url}: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("{url}: HTTP {}", response.status()));
        }
        response
            .json::<T>()
            .await
            .map_err(|e| format!("{url}: {e}"))
    }

    /// Discovery and keys for `issuer`, from the cache unless `refresh_keys`.
    async fn provider_data(
        &self,
        issuer: &str,
        refresh_keys: bool,
    ) -> Result<(Discovery, Vec<Jwk>), String> {
        if !refresh_keys
            && let Some(c) = self.cache.read().await.get(issuer)
            && c.at.elapsed() < CACHE_FOR
        {
            return Ok((c.discovery.clone(), c.keys.clone()));
        }
        let discovery: Discovery = self
            .get_json(&format!("{issuer}/.well-known/openid-configuration"))
            .await?;
        // Mix-up protection: the document must describe the issuer we asked about.
        if discovery.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
            return Err(format!(
                "discovery for {issuer} names issuer {}",
                discovery.issuer
            ));
        }
        let jwks: Jwks = self.get_json(&discovery.jwks_uri).await?;
        self.cache.write().await.insert(
            issuer.to_owned(),
            Cached {
                at: Instant::now(),
                discovery: discovery.clone(),
                keys: jwks.keys.clone(),
            },
        );
        Ok((discovery, jwks.keys))
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
}

#[async_trait]
impl OidcClient for HttpOidcClient {
    async fn authorization_url(
        &self,
        provider: &SsoProvider,
        redirect_uri: &str,
        state: &str,
        nonce: &str,
        code_challenge: &str,
    ) -> Result<String, String> {
        let i = provider.input();
        let (discovery, _) = self.provider_data(&i.issuer_url, false).await?;
        let url = reqwest::Url::parse_with_params(
            &discovery.authorization_endpoint,
            &[
                ("response_type", "code"),
                ("client_id", i.client_id.as_str()),
                ("redirect_uri", redirect_uri),
                ("scope", i.scopes.as_str()),
                ("state", state),
                ("nonce", nonce),
                ("code_challenge", code_challenge),
                ("code_challenge_method", "S256"),
            ],
        )
        .map_err(|e| format!("authorization endpoint: {e}"))?;
        Ok(url.to_string())
    }

    async fn exchange(
        &self,
        provider: &SsoProvider,
        client_secret: Option<&str>,
        redirect_uri: &str,
        code: &str,
        pkce_verifier: &str,
    ) -> Result<IdClaims, String> {
        let i = provider.input();
        let (discovery, keys) = self.provider_data(&i.issuer_url, false).await?;
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", i.client_id.as_str()),
            ("code_verifier", pkce_verifier),
        ];
        if let Some(secret) = client_secret {
            form.push(("client_secret", secret));
        }
        let response = self
            .http
            .post(&discovery.token_endpoint)
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("token endpoint: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("token endpoint: HTTP {}", response.status()));
        }
        let tokens: TokenResponse = response
            .json()
            .await
            .map_err(|e| format!("token endpoint: {e}"))?;
        let id_token = tokens.id_token.ok_or("the provider returned no ID token")?;
        match verify_id_token(&id_token, &keys) {
            Err(VerifyError::UnknownKey) => {
                let (_, fresh) = self.provider_data(&i.issuer_url, true).await?;
                verify_id_token(&id_token, &fresh).map_err(|e| e.to_string())
            }
            other => other.map_err(|e| e.to_string()),
        }
    }
}

/// Why an ID token was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    /// Not three base64url parts with JSON header and payload.
    #[error("the ID token is malformed")]
    Malformed,
    /// An algorithm other than RS256 or ES256.
    #[error("the ID token algorithm {0} is not accepted")]
    Algorithm(String),
    /// No provider key matches the token's key ID and algorithm.
    #[error("no provider key matches the ID token")]
    UnknownKey,
    /// The signature does not verify.
    #[error("the ID token signature is invalid")]
    BadSignature,
}

fn b64(part: &str) -> Result<Vec<u8>, VerifyError> {
    URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| VerifyError::Malformed)
}

/// Verifies a compact JWS ID token against `keys` and returns its claims. Claims are
/// checked separately ([`crate::domain::check_claims`]).
///
/// # Errors
///
/// See [`VerifyError`].
pub fn verify_id_token(token: &str, keys: &[Jwk]) -> Result<IdClaims, VerifyError> {
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(VerifyError::Malformed);
    };
    let header: Value = serde_json::from_slice(&b64(h)?).map_err(|_| VerifyError::Malformed)?;
    let alg = header["alg"].as_str().ok_or(VerifyError::Malformed)?;
    let kid = header["kid"].as_str();
    let signed = format!("{h}.{p}");
    let signature = b64(s)?;
    let candidates = keys.iter().filter(|k| {
        k.usage.as_deref().is_none_or(|u| u == "sig") && (kid.is_none() || k.kid.as_deref() == kid)
    });
    let mut matched = false;
    for key in candidates {
        let ok = match (alg, key.kty.as_str()) {
            ("RS256", "RSA") => {
                let (Some(n), Some(e)) = (key.n.as_deref(), key.e.as_deref()) else {
                    continue;
                };
                matched = true;
                ring::signature::RsaPublicKeyComponents {
                    n: b64(n)?,
                    e: b64(e)?,
                }
                .verify(
                    &ring::signature::RSA_PKCS1_2048_8192_SHA256,
                    signed.as_bytes(),
                    &signature,
                )
                .is_ok()
            }
            ("ES256", "EC") if key.crv.as_deref() == Some("P-256") => {
                let (Some(x), Some(y)) = (key.x.as_deref(), key.y.as_deref()) else {
                    continue;
                };
                matched = true;
                let mut point = vec![0x04];
                point.extend(b64(x)?);
                point.extend(b64(y)?);
                ring::signature::UnparsedPublicKey::new(
                    &ring::signature::ECDSA_P256_SHA256_FIXED,
                    point,
                )
                .verify(signed.as_bytes(), &signature)
                .is_ok()
            }
            ("RS256" | "ES256", _) => continue,
            (other, _) => return Err(VerifyError::Algorithm(other.to_owned())),
        };
        if ok {
            let payload: Value =
                serde_json::from_slice(&b64(p)?).map_err(|_| VerifyError::Malformed)?;
            return claims(&payload);
        }
    }
    if !matches!(alg, "RS256" | "ES256") {
        return Err(VerifyError::Algorithm(alg.to_owned()));
    }
    Err(if matched {
        VerifyError::BadSignature
    } else {
        VerifyError::UnknownKey
    })
}

fn claims(v: &Value) -> Result<IdClaims, VerifyError> {
    let text = |k: &str| v[k].as_str().map(str::to_owned);
    let aud = match &v["aud"] {
        Value::String(a) => vec![a.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => return Err(VerifyError::Malformed),
    };
    Ok(IdClaims {
        iss: text("iss").ok_or(VerifyError::Malformed)?,
        sub: text("sub").ok_or(VerifyError::Malformed)?,
        aud,
        azp: text("azp"),
        exp: v["exp"].as_i64().ok_or(VerifyError::Malformed)?,
        iat: v["iat"].as_i64(),
        nonce: text("nonce"),
        email: text("email"),
        // Some providers send the string "true".
        email_verified: v["email_verified"].as_bool().unwrap_or(false)
            || v["email_verified"] == "true",
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _};
    use serde_json::json;

    struct Key {
        pair: EcdsaKeyPair,
        jwk: Jwk,
    }

    fn key(kid: &str) -> Key {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        let point = pair.public_key().as_ref();
        let jwk = Jwk {
            kty: "EC".into(),
            kid: Some(kid.into()),
            usage: Some("sig".into()),
            n: None,
            e: None,
            crv: Some("P-256".into()),
            x: Some(URL_SAFE_NO_PAD.encode(&point[1..33])),
            y: Some(URL_SAFE_NO_PAD.encode(&point[33..65])),
        };
        Key { pair, jwk }
    }

    fn sign(k: &Key, header: &Value, payload: &Value) -> String {
        let h = URL_SAFE_NO_PAD.encode(header.to_string());
        let p = URL_SAFE_NO_PAD.encode(payload.to_string());
        let sig = k
            .pair
            .sign(&SystemRandom::new(), format!("{h}.{p}").as_bytes())
            .unwrap();
        format!("{h}.{p}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
    }

    fn payload() -> Value {
        json!({ "iss": "https://idp", "sub": "u1", "aud": "cooper", "exp": 2_000_000_000, "nonce": "n",
                "email": "ada@example.com", "email_verified": "true" })
    }

    #[test]
    fn es256_tokens_verify_against_the_matching_key_only() {
        let (good, other) = (key("k1"), key("k2"));
        let token = sign(&good, &json!({ "alg": "ES256", "kid": "k1" }), &payload());
        let claims = verify_id_token(&token, &[other.jwk.clone(), good.jwk.clone()]).unwrap();
        assert_eq!(claims.sub, "u1");
        assert_eq!(claims.aud, ["cooper"]);
        assert!(claims.email_verified);

        assert_eq!(
            verify_id_token(&token, std::slice::from_ref(&other.jwk)),
            Err(VerifyError::UnknownKey)
        );
        let mut wrong = other.jwk.clone();
        wrong.kid = Some("k1".into());
        assert_eq!(
            verify_id_token(&token, &[wrong]),
            Err(VerifyError::BadSignature)
        );

        // Tampered payload.
        let parts: Vec<&str> = token.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(json!({ "sub": "admin" }).to_string());
        let tampered = format!("{}.{forged}.{}", parts[0], parts[2]);
        assert_eq!(
            verify_id_token(&tampered, std::slice::from_ref(&good.jwk)),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn none_and_hmac_are_refused() {
        let k = key("k1");
        for alg in ["none", "HS256", "RS512"] {
            let token = sign(&k, &json!({ "alg": alg, "kid": "k1" }), &payload());
            assert_eq!(
                verify_id_token(&token, std::slice::from_ref(&k.jwk)),
                Err(VerifyError::Algorithm(alg.into()))
            );
        }
        assert_eq!(
            verify_id_token("a.b", &[k.jwk]),
            Err(VerifyError::Malformed)
        );
    }
}
