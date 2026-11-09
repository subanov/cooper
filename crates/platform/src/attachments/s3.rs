//! S3-compatible blob store backend (ADR-0007, ADR-0033): AWS S3, MinIO, Ceph RGW and
//! other services that speak the S3 REST API.
//!
//! Requests are signed with AWS Signature Version 4 over the existing `hmac`/`sha2`
//! crates and sent with `reqwest` (rustls, ring, Mozilla roots), so no SDK is needed.
//! Objects are private: Cooper never hands out bucket URLs, every download goes through
//! the API (ADR-0007). Credentials never appear in logs or error messages.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use super::{BlobBackend, BlobEntry, BlobError, BlobPage, blob_key, is_sha256};
use crate::config::{self, ConfigError, Secret};

/// SHA-256 of an empty payload.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Where and how to reach the bucket.
#[derive(Debug, Clone)]
pub struct S3Config {
    /// Service endpoint, for example `https://s3.eu-west-1.amazonaws.com` or
    /// `http://localhost:9000` (MinIO).
    pub endpoint: String,
    /// Signing region, for example `eu-west-1` (`us-east-1` for MinIO).
    pub region: String,
    /// Bucket name.
    pub bucket: String,
    /// Access key ID.
    pub access_key_id: String,
    /// Secret access key.
    pub secret_access_key: Secret,
    /// Optional key prefix inside the bucket, for example `cooper/`.
    pub prefix: String,
    /// Path-style addressing (`<endpoint>/<bucket>/<key>`), needed by MinIO and most
    /// self-hosted services. Otherwise virtual-hosted (`<bucket>.<endpoint host>`).
    pub path_style: bool,
}

impl S3Config {
    /// Reads `COOPER_S3_ENDPOINT`, `COOPER_S3_REGION`, `COOPER_S3_BUCKET`,
    /// `COOPER_S3_ACCESS_KEY_ID`, `COOPER_S3_SECRET_ACCESS_KEY`, `COOPER_S3_PREFIX` and
    /// `COOPER_S3_PATH_STYLE`.
    ///
    /// # Errors
    ///
    /// A required variable is missing or invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        let endpoint = config::required("COOPER_S3_ENDPOINT")?;
        let url = reqwest::Url::parse(endpoint.trim()).map_err(|e| ConfigError::Invalid {
            name: "COOPER_S3_ENDPOINT",
            reason: e.to_string(),
        })?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(ConfigError::Invalid {
                name: "COOPER_S3_ENDPOINT",
                reason: "must be an http:// or https:// URL".to_owned(),
            });
        }
        let mut prefix = config::var("COOPER_S3_PREFIX")?
            .unwrap_or_default()
            .trim()
            .trim_matches('/')
            .to_owned();
        if !prefix.is_empty() {
            prefix.push('/');
        }
        Ok(Self {
            endpoint: endpoint.trim().trim_end_matches('/').to_owned(),
            region: config::parse_or("COOPER_S3_REGION", "us-east-1".to_owned())?,
            bucket: config::required("COOPER_S3_BUCKET")?,
            access_key_id: config::required("COOPER_S3_ACCESS_KEY_ID")?,
            secret_access_key: Secret::new(config::required("COOPER_S3_SECRET_ACCESS_KEY")?),
            prefix,
            path_style: config::parse_or("COOPER_S3_PATH_STYLE", false)?,
        })
    }
}

/// Content stored in an S3-compatible bucket.
#[derive(Clone)]
pub struct S3BlobStore {
    config: Arc<S3Config>,
    http: reqwest::Client,
}

impl std::fmt::Debug for S3BlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3BlobStore")
            .field("endpoint", &self.config.endpoint)
            .field("bucket", &self.config.bucket)
            .finish_non_exhaustive()
    }
}

/// A request ready to sign.
struct Signed {
    url: String,
    headers: Vec<(&'static str, String)>,
}

impl S3BlobStore {
    /// A store for `config`.
    ///
    /// # Errors
    ///
    /// The HTTP client cannot be built.
    pub fn new(config: S3Config) -> Result<Self, BlobError> {
        let roots = tokio_rustls::rustls::RootCertStore::from_iter(
            webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
        );
        let tls = tokio_rustls::rustls::ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| BlobError::Backend(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .user_agent(concat!("Cooper/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| BlobError::Backend(e.to_string()))?;
        Ok(Self {
            config: Arc::new(config),
            http,
        })
    }

    /// `(url without query, host header, canonical path)` for an object key (or the
    /// bucket itself with `None`).
    fn locate(&self, key: Option<&str>) -> Result<(String, String, String), BlobError> {
        let base = reqwest::Url::parse(&self.config.endpoint)
            .map_err(|e| BlobError::Backend(e.to_string()))?;
        let scheme = base.scheme();
        let host = base
            .host_str()
            .ok_or_else(|| BlobError::Backend("endpoint has no host".into()))?;
        let port = base.port().map(|p| format!(":{p}")).unwrap_or_default();
        let base_path = base.path().trim_end_matches('/');
        let key_path = key.map(|k| format!("/{}", uri_encode(k, false)));
        let (host, path) = if self.config.path_style {
            (
                format!("{host}{port}"),
                format!(
                    "{base_path}/{}{}",
                    uri_encode(&self.config.bucket, true),
                    key_path.unwrap_or_default()
                ),
            )
        } else {
            (
                format!("{}.{host}{port}", self.config.bucket),
                format!("{base_path}{}", key_path.unwrap_or_else(|| "/".to_owned())),
            )
        };
        Ok((format!("{scheme}://{host}{path}"), host, path))
    }

    fn sign(
        &self,
        method: &str,
        key: Option<&str>,
        query: &[(&str, &str)],
        payload_sha256: &str,
        now: DateTime<Utc>,
    ) -> Result<Signed, BlobError> {
        let (url, host, path) = self.locate(key)?;
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let headers = vec![
            ("host", host),
            ("x-amz-content-sha256", payload_sha256.to_owned()),
            ("x-amz-date", amz_date),
        ];
        let authorization = authorization(
            &SigningKey {
                access_key_id: &self.config.access_key_id,
                secret_access_key: self.config.secret_access_key.expose(),
                region: &self.config.region,
            },
            method,
            &path,
            query,
            &headers,
            payload_sha256,
            now,
        );
        let query_string = canonical_query(query);
        let url = if query_string.is_empty() {
            url
        } else {
            format!("{url}?{query_string}")
        };
        let mut headers: Vec<(&'static str, String)> = headers
            .into_iter()
            .filter(|(name, _)| *name != "host")
            .collect();
        headers.push(("authorization", authorization));
        Ok(Signed { url, headers })
    }

    fn request(&self, method: reqwest::Method, signed: Signed) -> reqwest::RequestBuilder {
        signed
            .headers
            .into_iter()
            .fold(self.http.request(method, signed.url), |r, (k, v)| {
                r.header(k, v)
            })
    }

    fn key(&self, sha256: &str) -> Result<String, BlobError> {
        if !is_sha256(sha256) {
            return Err(BlobError::NotFound);
        }
        Ok(format!("{}{}", self.config.prefix, blob_key(sha256)))
    }
}

/// A failed S3 call, without URLs or credentials.
fn failure(what: &str, status: reqwest::StatusCode, body: &str) -> BlobError {
    let code = tag_text(body, "Code").unwrap_or_default();
    BlobError::Backend(
        format!("S3 {what} failed: HTTP {status} {code}")
            .trim()
            .to_owned(),
    )
}

fn transport(what: &str, e: &reqwest::Error) -> BlobError {
    let mut text = e.to_string();
    if let Some(i) = text.find(" for url (") {
        text.truncate(i);
    }
    BlobError::Backend(format!("S3 {what} failed: {text}"))
}

#[async_trait]
impl BlobBackend for S3BlobStore {
    fn kind(&self) -> &'static str {
        "s3"
    }

    async fn put(&self, sha256: &str, path: &Path, size: u64) -> Result<(), BlobError> {
        let key = self.key(sha256)?;
        // Always upload, even when the object exists: identical content, and its
        // Last-Modified is refreshed, which keeps it out of the orphan clean-up's
        // grace period while the attachment row is written (ADR-0033).
        let file = tokio::fs::File::open(path).await?;
        let signed = self.sign("PUT", Some(&key), &[], sha256, Utc::now())?;
        let response = self
            .request(reqwest::Method::PUT, signed)
            .header(reqwest::header::CONTENT_LENGTH, size)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(reqwest::Body::from(file))
            .send()
            .await
            .map_err(|e| transport("upload", &e))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(failure("upload", status, &body));
        }
        let _ = tokio::fs::remove_file(path).await;
        Ok(())
    }

    async fn get(&self, sha256: &str) -> Result<Vec<u8>, BlobError> {
        let key = self.key(sha256)?;
        let signed = self.sign("GET", Some(&key), &[], EMPTY_SHA256, Utc::now())?;
        let response = self
            .request(reqwest::Method::GET, signed)
            .send()
            .await
            .map_err(|e| transport("download", &e))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(BlobError::NotFound);
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(failure("download", status, &body));
        }
        Ok(response
            .bytes()
            .await
            .map_err(|e| transport("download", &e))?
            .to_vec())
    }

    async fn modified(&self, sha256: &str) -> Result<Option<DateTime<Utc>>, BlobError> {
        let key = self.key(sha256)?;
        let signed = self.sign("HEAD", Some(&key), &[], EMPTY_SHA256, Utc::now())?;
        let response = self
            .request(reqwest::Method::HEAD, signed)
            .send()
            .await
            .map_err(|e| transport("lookup", &e))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(failure("lookup", status, ""));
        }
        let modified = response
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| DateTime::parse_from_rfc2822(v).ok())
            .map(|d| d.with_timezone(&Utc))
            .ok_or_else(|| BlobError::Backend("S3 lookup: no Last-Modified".into()))?;
        Ok(Some(modified))
    }

    async fn delete(&self, sha256: &str) -> Result<(), BlobError> {
        let key = self.key(sha256)?;
        let signed = self.sign("DELETE", Some(&key), &[], EMPTY_SHA256, Utc::now())?;
        let response = self
            .request(reqwest::Method::DELETE, signed)
            .send()
            .await
            .map_err(|e| transport("delete", &e))?;
        let status = response.status();
        if status.is_success() || status == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(failure("delete", status, &body))
    }

    async fn list(&self, page: Option<String>) -> Result<BlobPage, BlobError> {
        let prefix = format!("{}sha256/", self.config.prefix);
        let mut query = vec![
            ("list-type", "2"),
            ("max-keys", "1000"),
            ("prefix", prefix.as_str()),
        ];
        if let Some(token) = page.as_deref() {
            query.push(("continuation-token", token));
        }
        let signed = self.sign("GET", None, &query, EMPTY_SHA256, Utc::now())?;
        let response = self
            .request(reqwest::Method::GET, signed)
            .send()
            .await
            .map_err(|e| transport("listing", &e))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| transport("listing", &e))?;
        if !status.is_success() {
            return Err(failure("listing", status, &body));
        }
        Ok(parse_list(&body))
    }
}

/// Parses a `ListObjectsV2` answer: keys ending in a SHA-256 with their modification
/// time, and the continuation token when the listing is truncated.
fn parse_list(xml: &str) -> BlobPage {
    let mut blobs = Vec::new();
    for block in xml.split("<Contents>").skip(1) {
        let block = block.split("</Contents>").next().unwrap_or_default();
        let (Some(key), Some(modified)) = (tag_text(block, "Key"), tag_text(block, "LastModified"))
        else {
            continue;
        };
        let sha256 = key.rsplit('/').next().unwrap_or_default();
        let Ok(modified) = DateTime::parse_from_rfc3339(&modified) else {
            continue;
        };
        if is_sha256(sha256) {
            blobs.push(BlobEntry {
                sha256: sha256.to_owned(),
                modified: modified.with_timezone(&Utc),
            });
        }
    }
    let truncated = tag_text(xml, "IsTruncated").is_some_and(|t| t == "true");
    BlobPage {
        blobs,
        next: truncated
            .then(|| tag_text(xml, "NextContinuationToken"))
            .flatten(),
    }
}

/// The unescaped text of the first `<tag>…</tag>` in `xml`.
fn tag_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(
        xml[start..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

/// Credentials and scope of a signature.
struct SigningKey<'a> {
    access_key_id: &'a str,
    secret_access_key: &'a str,
    region: &'a str,
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    // HMAC accepts keys of any length, so this never fails in practice.
    let Ok(mut mac) = <Hmac<Sha256> as KeyInit>::new_from_slice(key) else {
        return Vec::new();
    };
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// RFC 3986 encoding as SigV4 wants it: unreserved characters stay, everything else is
/// `%XX`; `/` stays in paths.
fn uri_encode(s: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~')
            || (b == b'/' && !encode_slash)
        {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn canonical_query(query: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k, true), uri_encode(v, true)))
        .collect();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// The `Authorization` header of AWS Signature Version 4 for service `s3`. `headers`
/// are lower-case names with their values and must include `host` and `x-amz-date`.
fn authorization(
    key: &SigningKey<'_>,
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    headers: &[(&str, String)],
    payload_sha256: &str,
    now: DateTime<Utc>,
) -> String {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let mut sorted: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.trim())).collect();
    sorted.sort_by_key(|(k, _)| *k);
    let canonical_headers: String = sorted.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers = sorted.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(";");
    let canonical_request = format!(
        "{method}\n{path}\n{}\n{canonical_headers}\n{signed_headers}\n{payload_sha256}",
        canonical_query(query)
    );
    let scope = format!("{date}/{}/s3/aws4_request", key.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );
    let k_date = hmac(format!("AWS4{}", key.secret_access_key).as_bytes(), &date);
    let k_region = hmac(&k_date, key.region);
    let k_service = hmac(&k_region, "s3");
    let k_signing = hmac(&k_service, "aws4_request");
    let signature = hex::encode(hmac(&k_signing, &string_to_sign));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        key.access_key_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const EXAMPLE: SigningKey<'static> = SigningKey {
        access_key_id: "AKIAIOSFODNN7EXAMPLE",
        secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        region: "us-east-1",
    };

    fn example_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap()
    }

    /// The "GET Object" example of the AWS Signature Version 4 documentation.
    #[test]
    fn signs_the_aws_get_object_example() {
        let headers = [
            ("host", "examplebucket.s3.amazonaws.com".to_owned()),
            ("range", "bytes=0-9".to_owned()),
            ("x-amz-content-sha256", EMPTY_SHA256.to_owned()),
            ("x-amz-date", "20130524T000000Z".to_owned()),
        ];
        let auth = authorization(
            &EXAMPLE,
            "GET",
            "/test.txt",
            &[],
            &headers,
            EMPTY_SHA256,
            example_time(),
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    /// The "GET Bucket (List Objects)" example: the query is sorted and encoded.
    #[test]
    fn signs_the_aws_list_objects_example() {
        let headers = [
            ("host", "examplebucket.s3.amazonaws.com".to_owned()),
            ("x-amz-content-sha256", EMPTY_SHA256.to_owned()),
            ("x-amz-date", "20130524T000000Z".to_owned()),
        ];
        let auth = authorization(
            &EXAMPLE,
            "GET",
            "/",
            &[("prefix", "J"), ("max-keys", "2")],
            &headers,
            EMPTY_SHA256,
            example_time(),
        );
        assert!(
            auth.ends_with(
                "Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
            ),
            "{auth}"
        );
    }

    fn store(path_style: bool) -> S3BlobStore {
        S3BlobStore::new(S3Config {
            endpoint: "http://localhost:9000".into(),
            region: "us-east-1".into(),
            bucket: "cooper".into(),
            access_key_id: "id".into(),
            secret_access_key: Secret::new("very-secret".into()),
            prefix: "p/".into(),
            path_style,
        })
        .unwrap()
    }

    #[test]
    fn objects_are_addressed_path_or_host_style_without_leaking_secrets() {
        let key = format!("p/{}", blob_key(&"a".repeat(64)));
        let (url, host, path) = store(true).locate(Some(&key)).unwrap();
        assert_eq!(host, "localhost:9000");
        assert_eq!(path, format!("/cooper/{key}"));
        assert_eq!(url, format!("http://localhost:9000/cooper/{key}"));
        let (url, host, _) = store(false).locate(Some(&key)).unwrap();
        assert_eq!(host, "cooper.localhost:9000");
        assert_eq!(url, format!("http://cooper.localhost:9000/{key}"));
        assert!(!format!("{:?}", store(true)).contains("very-secret"));
        let signed = store(true)
            .sign("GET", Some(&key), &[], EMPTY_SHA256, example_time())
            .unwrap();
        assert!(
            signed
                .headers
                .iter()
                .all(|(_, v)| !v.contains("very-secret"))
        );
        assert!(matches!(
            store(true).key("../etc"),
            Err(BlobError::NotFound)
        ));
    }

    #[test]
    fn listings_are_parsed() {
        let a = "a".repeat(64);
        let xml = format!(
            "<?xml version=\"1.0\"?><ListBucketResult><IsTruncated>true</IsTruncated>\
             <Contents><Key>p/sha256/aa/aa/{a}</Key><LastModified>2026-11-01T10:00:00.000Z</LastModified></Contents>\
             <Contents><Key>p/sha256/tmp/x</Key><LastModified>2026-11-01T10:00:00.000Z</LastModified></Contents>\
             <NextContinuationToken>1x&amp;2</NextContinuationToken></ListBucketResult>"
        );
        let page = parse_list(&xml);
        assert_eq!(page.blobs.len(), 1);
        assert_eq!(page.blobs[0].sha256, a);
        assert_eq!(page.next.as_deref(), Some("1x&2"));
        assert_eq!(
            parse_list("<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>")
                .next,
            None
        );
        assert_eq!(
            tag_text("<Error><Code>NoSuchBucket</Code></Error>", "Code").as_deref(),
            Some("NoSuchBucket")
        );
    }

    /// Against a real S3-compatible service when `COOPER_TEST_S3_ENDPOINT` is set (for
    /// example MinIO from `docker compose --profile s3 up -d`); skipped otherwise.
    #[tokio::test]
    async fn round_trip_against_a_live_service_when_configured() {
        let Some(endpoint) = std::env::var("COOPER_TEST_S3_ENDPOINT")
            .ok()
            .filter(|v| !v.trim().is_empty())
        else {
            return;
        };
        let env =
            |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_owned());
        let backend = S3BlobStore::new(S3Config {
            endpoint,
            region: env("COOPER_TEST_S3_REGION", "us-east-1"),
            bucket: env("COOPER_TEST_S3_BUCKET", "cooper-test"),
            access_key_id: env("COOPER_TEST_S3_ACCESS_KEY_ID", "cooper"),
            secret_access_key: Secret::new(env(
                "COOPER_TEST_S3_SECRET_ACCESS_KEY",
                "cooper-secret",
            )),
            prefix: format!("test-{}/", uuid::Uuid::now_v7()),
            path_style: true,
        })
        .unwrap();
        // Create the bucket if needed (409 when it exists).
        let signed = backend
            .sign("PUT", None, &[], EMPTY_SHA256, Utc::now())
            .unwrap();
        let created = backend
            .request(reqwest::Method::PUT, signed)
            .send()
            .await
            .unwrap();
        assert!(
            created.status().is_success() || created.status() == reqwest::StatusCode::CONFLICT,
            "{}",
            created.status()
        );
        let spool = std::env::temp_dir().join(format!("cooper-s3-{}", uuid::Uuid::now_v7()));
        let store = super::super::BlobStore::new(&spool, Arc::new(backend.clone()));
        let mut w = store.writer(1024).await.unwrap();
        w.write(b"hello s3").await.unwrap();
        let blob = w.finish().await.unwrap();
        assert_eq!(store.read(&blob.sha256).await.unwrap(), b"hello s3");
        assert!(backend.modified(&blob.sha256).await.unwrap().is_some());
        let page = backend.list(None).await.unwrap();
        assert!(page.blobs.iter().any(|b| b.sha256 == blob.sha256));
        backend.delete(&blob.sha256).await.unwrap();
        assert!(matches!(
            store.read(&blob.sha256).await,
            Err(BlobError::NotFound)
        ));
        assert!(backend.modified(&blob.sha256).await.unwrap().is_none());
        let _ = tokio::fs::remove_dir_all(spool).await;
    }
}
