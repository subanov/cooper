//! Suggested Replies through the Google Gemini API (FR-AI-003, ADR-0037), behind the
//! `ai` feature.
//!
//! One `generateContent` call per model: the configured model first, then each fallback
//! when a model is overloaded, rate limited, unknown or too slow. Everything must finish
//! within one deadline, shorter than the server's request timeout.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::oauth::HttpClient;
use crate::domain::{ReplySuggester, SuggestError, SuggestedReply, SuggestedReplyRequest};

/// The public Gemini API.
pub const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Gemini settings (`GEMINI_*` variables).
#[derive(Clone)]
pub struct GeminiConfig {
    /// API key (`GEMINI_API_KEY`).
    pub api_key: String,
    /// Model tried first (`GEMINI_MODEL`).
    pub model: String,
    /// Models tried next, in order (`GEMINI_FALLBACK_MODELS`).
    pub fallback_models: Vec<String>,
    /// API base URL (tests use a local server).
    pub base_url: String,
    /// Time allowed for all attempts together.
    pub deadline: Duration,
    /// Time allowed for one model.
    pub attempt_timeout: Duration,
}

impl GeminiConfig {
    /// Settings for the public API with the default timeouts.
    #[must_use]
    pub fn new(api_key: &str, model: &str, fallback_models: Vec<String>) -> Self {
        Self {
            api_key: api_key.trim().to_owned(),
            model: model.trim().to_owned(),
            fallback_models,
            base_url: GEMINI_API_BASE.to_owned(),
            deadline: Duration::from_secs(25),
            attempt_timeout: Duration::from_secs(15),
        }
    }
}

impl std::fmt::Debug for GeminiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiConfig")
            .field("api_key", &"[redacted]")
            .field("model", &self.model)
            .field("fallback_models", &self.fallback_models)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

/// Suggests Replies with Gemini.
#[derive(Clone)]
pub struct GeminiSuggester {
    http: HttpClient,
    config: GeminiConfig,
}

impl std::fmt::Debug for GeminiSuggester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiSuggester")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl GeminiSuggester {
    /// A client for `config`.
    ///
    /// # Errors
    ///
    /// No key or model, or the HTTP client cannot be built.
    pub fn new(config: GeminiConfig) -> Result<Self, String> {
        if config.api_key.is_empty() || config.model.is_empty() {
            return Err("GEMINI_API_KEY and GEMINI_MODEL must not be empty".into());
        }
        Ok(Self {
            http: HttpClient::new()?,
            config: GeminiConfig {
                base_url: config.base_url.trim_end_matches('/').to_owned(),
                ..config
            },
        })
    }

    /// The models in the order they are tried.
    fn models(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.config.model.as_str()).chain(
            self.config
                .fallback_models
                .iter()
                .map(String::as_str)
                .filter(|m| !m.is_empty() && *m != self.config.model),
        )
    }

    async fn call(
        &self,
        model: &str,
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<SuggestedReply, Attempt> {
        let url = format!("{}/models/{model}:generateContent", self.config.base_url);
        let response = self
            .http
            .client()
            .post(url)
            .header("x-goog-api-key", &self.config.api_key)
            .timeout(timeout)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                Attempt::TryNext(SuggestError::Unavailable(if e.is_timeout() {
                    format!("{model}: no answer within {}s", timeout.as_secs())
                } else {
                    format!("{model}: {}", e.without_url())
                }))
            })?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            let detail = api_message(&detail).unwrap_or_else(|| status.to_string());
            let reason = format!("{model}: HTTP {}: {detail}", status.as_u16());
            return Err(match status.as_u16() {
                404 => Attempt::TryNext(SuggestError::Misconfigured(reason)),
                408 | 429 | 500..=599 => Attempt::TryNext(SuggestError::Unavailable(reason)),
                _ => Attempt::Stop(SuggestError::Misconfigured(reason)),
            });
        }
        let answer: GenerateResponse = response.json().await.map_err(|e| {
            Attempt::TryNext(SuggestError::Unavailable(format!(
                "{model}: unreadable answer: {}",
                e.without_url()
            )))
        })?;
        answer.into_reply(model).map_err(Attempt::Stop)
    }
}

/// The outcome of one failed model.
enum Attempt {
    /// Another model may still answer.
    TryNext(SuggestError),
    /// No other model will do better.
    Stop(SuggestError),
}

#[async_trait]
impl ReplySuggester for GeminiSuggester {
    async fn suggest(
        &self,
        request: &SuggestedReplyRequest,
    ) -> Result<SuggestedReply, SuggestError> {
        let body = json!({
            "systemInstruction": {"parts": [{"text": request.system()}]},
            "contents": [{"role": "user", "parts": [{"text": request.content()}]}],
            "generationConfig": {"temperature": 0.3},
        });
        let started = Instant::now();
        let deadline = request.time_limit().map_or(self.config.deadline, |limit| {
            limit.min(self.config.deadline)
        });
        let mut failure: Option<SuggestError> = None;
        for model in self.models() {
            let left = deadline.saturating_sub(started.elapsed());
            if left < Duration::from_secs(1) {
                break;
            }
            match self
                .call(model, &body, left.min(self.config.attempt_timeout))
                .await
            {
                Ok(reply) => return Ok(reply),
                Err(Attempt::Stop(err)) => return Err(err),
                Err(Attempt::TryNext(err)) => {
                    tracing::info!(model, reason = %err, "Gemini model failed; trying the next one");
                    // An overloaded model is the likelier story than an unknown one.
                    if !matches!(failure, Some(SuggestError::Unavailable(_))) {
                        failure = Some(err);
                    }
                }
            }
        }
        Err(failure
            .unwrap_or_else(|| SuggestError::Unavailable("no answer before the deadline".into())))
    }
}

/// The `error.message` of a Google API error body.
fn api_message(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Envelope {
        error: Inner,
    }
    #[derive(Deserialize)]
    struct Inner {
        message: String,
    }
    serde_json::from_str::<Envelope>(body)
        .ok()
        .map(|e| e.error.message.chars().take(300).collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default)]
    prompt_feedback: Option<PromptFeedback>,
    #[serde(default)]
    model_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    #[serde(default)]
    content: Option<Content>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Content {
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
struct Part {
    #[serde(default)]
    text: Option<String>,
    /// Thought summaries of thinking models; not part of the answer.
    #[serde(default)]
    thought: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptFeedback {
    #[serde(default)]
    block_reason: Option<String>,
}

impl GenerateResponse {
    fn into_reply(self, model: &str) -> Result<SuggestedReply, SuggestError> {
        if let Some(reason) = self.prompt_feedback.and_then(|p| p.block_reason) {
            return Err(SuggestError::Rejected(format!("prompt blocked ({reason})")));
        }
        let Some(candidate) = self.candidates.into_iter().next() else {
            return Err(SuggestError::Rejected("no candidates".into()));
        };
        let text: String = candidate
            .content
            .map(|c| c.parts)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| !p.thought)
            .filter_map(|p| p.text)
            .collect();
        if text.trim().is_empty() {
            let reason = candidate.finish_reason.unwrap_or_else(|| "empty".into());
            return Err(SuggestError::Rejected(format!("no text ({reason})")));
        }
        SuggestedReply::new(&text, self.model_version.as_deref().unwrap_or(model))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::extract::{Path, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::post;
    use serde_json::Value;

    use super::*;
    use crate::domain::{ReplyContext, SuggestedReplyRequest};

    /// Per model: the status and body to answer with. Records the models asked and keys.
    #[derive(Clone, Default)]
    struct Mock {
        answers: Arc<Mutex<Vec<(String, u16, Value)>>>,
        calls: Arc<Mutex<Vec<(String, String, Value)>>>,
    }

    async fn generate(
        State(mock): State<Mock>,
        Path(target): Path<String>,
        headers: HeaderMap,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        let model = target.trim_end_matches(":generateContent").to_owned();
        let key = headers
            .get("x-goog-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        mock.calls.lock().unwrap().push((model.clone(), key, body));
        if model == "slow" {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
        let answer = mock
            .answers
            .lock()
            .unwrap()
            .iter()
            .find(|(m, _, _)| *m == model)
            .cloned();
        match answer {
            Some((_, status, body)) => {
                (StatusCode::from_u16(status).unwrap(), axum::Json(body)).into_response()
            }
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }

    async fn server(answers: Vec<(&str, u16, Value)>) -> (String, Mock) {
        let mock = Mock::default();
        *mock.answers.lock().unwrap() = answers
            .into_iter()
            .map(|(m, s, b)| (m.to_owned(), s, b))
            .collect();
        let app = Router::new()
            .route("/models/{target}", post(generate))
            .with_state(mock.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        (format!("http://{addr}"), mock)
    }

    fn suggester(base: &str) -> GeminiSuggester {
        GeminiSuggester::new(GeminiConfig {
            base_url: base.to_owned(),
            ..GeminiConfig::new("test-key", "first", vec!["second".into()])
        })
        .unwrap()
    }

    fn request() -> SuggestedReplyRequest {
        SuggestedReplyRequest::for_ticket(
            &crate::domain::SuggestedReplyInstructions::default(),
            &ReplyContext {
                ticket_number: "DEF-1",
                subject: "Printer",
                description: "It jams.",
                requester_name: "Rita",
                analyst_name: "Ann",
                summary: "",
            },
            &[],
            None,
        )
        .unwrap()
    }

    fn text(t: &str) -> Value {
        json!({
            "candidates": [{"content": {"parts": [{"text": "thinking", "thought": true}, {"text": t}]}, "finishReason": "STOP"}],
            "modelVersion": "first-001"
        })
    }

    #[tokio::test]
    async fn answers_with_the_first_model_and_sends_key_and_prompt() {
        let (base, mock) = server(vec![("first", 200, text("Hello Rita,"))]).await;
        let reply = suggester(&base).suggest(&request()).await.unwrap();
        assert_eq!(reply.text(), "Hello Rita,");
        assert_eq!(reply.model(), "first-001");
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "test-key");
        let body = &calls[0].2;
        assert!(
            body["systemInstruction"]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .contains("IT service desk")
        );
        assert!(
            body["contents"][0]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .contains("It jams.")
        );
    }

    #[tokio::test]
    async fn a_request_time_limit_shortens_the_deadline() {
        let (base, _) = server(vec![("slow", 200, text("too late"))]).await;
        let slow = GeminiSuggester::new(GeminiConfig {
            base_url: base,
            ..GeminiConfig::new("test-key", "slow", vec![])
        })
        .unwrap();
        let started = Instant::now();
        let err = slow
            .suggest(&request().with_time_limit(Duration::from_millis(1_500)))
            .await
            .unwrap_err();
        assert!(matches!(err, SuggestError::Unavailable(_)), "{err:?}");
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn overloaded_model_falls_back_to_the_next() {
        let (base, mock) = server(vec![
            ("first", 503, json!({"error": {"message": "high demand"}})),
            ("second", 200, text("From the fallback")),
        ])
        .await;
        let reply = suggester(&base).suggest(&request()).await.unwrap();
        assert_eq!(reply.text(), "From the fallback");
        assert_eq!(mock.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn all_models_overloaded_is_unavailable() {
        let (base, _) = server(vec![
            ("first", 429, json!({"error": {"message": "quota"}})),
            ("second", 503, json!({"error": {"message": "busy"}})),
        ])
        .await;
        let err = suggester(&base).suggest(&request()).await.unwrap_err();
        assert!(matches!(err, SuggestError::Unavailable(_)), "{err:?}");
    }

    #[tokio::test]
    async fn bad_key_stops_without_fallback() {
        let (base, mock) = server(vec![(
            "first",
            400,
            json!({"error": {"message": "API key not valid"}}),
        )])
        .await;
        let err = suggester(&base).suggest(&request()).await.unwrap_err();
        assert!(
            matches!(&err, SuggestError::Misconfigured(r) if r.contains("API key not valid")),
            "{err:?}"
        );
        assert_eq!(mock.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unknown_models_are_misconfigured() {
        let (base, _) = server(vec![]).await;
        let err = suggester(&base).suggest(&request()).await.unwrap_err();
        assert!(matches!(err, SuggestError::Misconfigured(_)), "{err:?}");
    }

    #[tokio::test]
    async fn safety_block_is_rejected() {
        let (base, mock) = server(vec![(
            "first",
            200,
            json!({"candidates": [{"finishReason": "SAFETY"}]}),
        )])
        .await;
        let err = suggester(&base).suggest(&request()).await.unwrap_err();
        assert!(
            matches!(&err, SuggestError::Rejected(r) if r.contains("SAFETY")),
            "{err:?}"
        );
        assert_eq!(mock.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn empty_candidates_are_rejected() {
        let (base, _) = server(vec![("first", 200, json!({"candidates": []}))]).await;
        let err = suggester(&base).suggest(&request()).await.unwrap_err();
        assert!(matches!(err, SuggestError::Rejected(_)), "{err:?}");
    }

    #[test]
    fn debug_output_hides_the_key() {
        let s = format!("{:?}", GeminiConfig::new("secret-key", "m", vec![]));
        assert!(!s.contains("secret-key"));
    }
}
