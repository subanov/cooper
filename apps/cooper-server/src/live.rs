//! Live Updates (ADR-0022, FR-GEN-010): `GET /api/v1/live`, a Server-Sent Events stream
//! that tells the signed-in Analyst's browser which cached data to read again.
//!
//! Events carry no record data, only what changed (`{"topic":"ticket","id":…}`), and
//! only for records the Analyst may read. The checks run in the application layer for
//! every change, as the connected Analyst, and the session is re-checked every
//! [`REVALIDATE_EVERY`].

use std::collections::{HashSet, VecDeque};
use std::convert::Infallible;
use std::time::Duration;

use axum::Extension;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use cooper_http::Authenticated;
use cooper_identity::api::{CurrentSession, session_cookie};
use cooper_identity::domain::SessionToken;
use cooper_kernel::{Principal, TicketId};
use cooper_platform::live::{LiveChange, LiveSignal, LiveSubscription};
use http::{HeaderMap, HeaderValue};
use serde_json::json;
use tokio::sync::watch;
use tokio::time::{Instant, Interval, MissedTickBehavior};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

/// How often an open stream re-checks the session, Roles and Company access.
pub const REVALIDATE_EVERY: Duration = Duration::from_secs(60);

/// Changes handled in one go; a larger burst becomes one `resync`.
const MAX_BATCH: usize = 256;

/// Topic of ticket changes.
const TICKET: &str = "ticket";
/// Topic of Ticket TicketPresence changes (FR-TKT-025); authorised like the ticket itself.
const PRESENCE: &str = cooper_service_desk::infrastructure::PRESENCE_TOPIC;

/// The Live Updates route, to be nested under `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(live_updates))
}

/// Live Updates for the signed-in Analyst (Server-Sent Events, ADR-0022).
///
/// Events: `ready` once changes are being received (refetch on every `ready` after the
/// first: changes may have been missed while disconnected); `invalidate` with
/// `{"topic":"ticket"|"ticket_presence","id":"…"}` when a ticket the Analyst may see
/// (or who has it open) changed; `resync`
/// when everything should be read again. Events never contain record data.
///
/// Needs the Analyst session cookie; API keys and Portal sessions are refused.
#[utoipa::path(
    get,
    path = "/live",
    tag = "Live updates",
    responses(
        (status = 200, description = "An endless `text/event-stream`", content_type = "text/event-stream", body = String),
        (status = 401, description = "Not signed in with an Analyst session"),
    )
)]
async fn live_updates(
    State(state): State<AppState>,
    Authenticated(principal): Authenticated,
    session: Option<Extension<CurrentSession>>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = session_cookie(&headers).filter(|_| session.is_some()) else {
        return cooper_http::auth::unauthenticated()
            .with_detail("Live Updates need a signed-in Analyst session.")
            .into_response();
    };
    let stream = LiveStream::new(&state, principal, token);
    let events = futures::stream::unfold(stream, |mut stream| async move {
        stream
            .next()
            .await
            .map(|event| (Ok::<_, Infallible>(event), stream))
    });
    let mut response = Sse::new(events)
        .keep_alive(KeepAlive::default())
        .into_response();
    // Reverse proxies (nginx) must not buffer the stream.
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

/// One open stream.
struct LiveStream {
    state: AppState,
    token: SessionToken,
    principal: Principal,
    subscription: LiveSubscription,
    listening: watch::Receiver<bool>,
    shutdown: watch::Receiver<bool>,
    revalidate: Interval,
    ready_sent: bool,
    pending: VecDeque<Event>,
}

/// Why [`LiveStream::next`] woke up.
enum Wake {
    Shutdown,
    Revalidate,
    Signal(Option<LiveSignal>),
}

/// Resolves once the server shuts down (or the feed is gone).
async fn shut_down(shutdown: &mut watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|s| *s).await;
}

/// What a batch of signals asks the browser to do.
enum Batch {
    Changes(Vec<LiveChange>),
    Resync,
}

impl LiveStream {
    fn new(state: &AppState, principal: Principal, token: SessionToken) -> Self {
        let mut revalidate =
            tokio::time::interval_at(Instant::now() + REVALIDATE_EVERY, REVALIDATE_EVERY);
        revalidate.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self {
            subscription: state.live.subscribe(),
            listening: state.live.listening(),
            shutdown: state.live.shutting_down(),
            state: state.clone(),
            token,
            principal,
            revalidate,
            ready_sent: false,
            pending: VecDeque::new(),
        }
    }

    /// The next event; `None` ends the stream (signed out, access lost or shutdown).
    async fn next(&mut self) -> Option<Event> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            if !self.ready_sent {
                let listening = tokio::select! {
                    () = shut_down(&mut self.shutdown) => false,
                    listening = self.listening.wait_for(|l| *l) => listening.is_ok(),
                };
                if !listening {
                    return None;
                }
                self.ready_sent = true;
                return Some(
                    Event::default()
                        .event("ready")
                        .retry(Duration::from_secs(5))
                        .data("{}"),
                );
            }
            let wake = tokio::select! {
                () = shut_down(&mut self.shutdown) => Wake::Shutdown,
                _ = self.revalidate.tick() => Wake::Revalidate,
                signal = self.subscription.recv() => Wake::Signal(signal),
            };
            match wake {
                Wake::Shutdown => return None,
                Wake::Revalidate => {
                    match self.state.identity.service.revalidate(&self.token).await {
                        Ok(principal) => self.principal = principal,
                        Err(error) => {
                            tracing::debug!(%error, "live updates stream ended: session no longer valid");
                            return None;
                        }
                    }
                }
                Wake::Signal(signal) => {
                    let batch = self.collect(signal?);
                    self.pending = self.events_for(batch).await;
                }
            }
        }
    }

    /// `first` plus every signal already waiting, de-duplicated, without changes the
    /// Analyst cannot possibly see (other Companies, unknown topics). No queries.
    fn collect(&mut self, first: LiveSignal) -> Batch {
        let mut seen = HashSet::new();
        let mut changes = Vec::new();
        let mut next = Some(first);
        let mut resync = false;
        while let Some(signal) = next {
            match signal {
                LiveSignal::Resync => resync = true,
                LiveSignal::Changed(change) => {
                    if !resync
                        && (change.topic == TICKET || change.topic == PRESENCE)
                        && self.principal.scope().allows(change.tenant_id)
                        && seen.insert(change.clone())
                    {
                        changes.push(change);
                        resync = changes.len() > MAX_BATCH;
                    }
                }
            }
            next = self.subscription.try_recv();
        }
        if resync {
            Batch::Resync
        } else {
            Batch::Changes(changes)
        }
    }

    /// The events to send for `batch`, after checking each change as the Analyst.
    async fn events_for(&self, batch: Batch) -> VecDeque<Event> {
        let changes = match batch {
            Batch::Resync => return VecDeque::from([resync()]),
            Batch::Changes(changes) => changes,
        };
        let mut events = VecDeque::new();
        for change in changes {
            let ticket = TicketId::from_uuid(change.id);
            match self
                .state
                .service_desk
                .service
                .can_see_ticket(&self.principal, ticket)
                .await
            {
                Ok(true) => events.push_back(
                    Event::default()
                        .event("invalidate")
                        .data(json!({ "topic": change.topic, "id": change.id }).to_string()),
                ),
                Ok(false) => {}
                Err(error) => {
                    // Visibility unknown: name no record, refetch everything instead.
                    tracing::warn!(%error, "live updates visibility check failed");
                    return VecDeque::from([resync()]);
                }
            }
        }
        events
    }
}

fn resync() -> Event {
    Event::default().event("resync").data("{}")
}
