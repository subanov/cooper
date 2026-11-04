//! Who performed an action.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::id::{AnalystId, ApiKeyId, RequesterId};

/// Whoever performed an action: an Analyst, a Requester, an API Key or Automation.
/// Recorded in every audit entry and domain event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Actor {
    /// A member of IT staff.
    Analyst(AnalystId),
    /// An end user, for example through the Self-Service Portal or email.
    Requester(RequesterId),
    /// An integration authenticated with an API Key.
    ApiKey(ApiKeyId),
    /// Cooper itself: background jobs, workflows, email intake.
    Automation,
}

/// The kind of an [`Actor`], as stored in the `actor_kind` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorKind {
    /// See [`Actor::Analyst`].
    Analyst,
    /// See [`Actor::Requester`].
    Requester,
    /// See [`Actor::ApiKey`].
    ApiKey,
    /// See [`Actor::Automation`].
    Automation,
}

impl ActorKind {
    /// Stable lower-case name used in storage and APIs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Analyst => "analyst",
            Self::Requester => "requester",
            Self::ApiKey => "api_key",
            Self::Automation => "automation",
        }
    }
}

impl fmt::Display for ActorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Stored actor columns do not describe a valid [`Actor`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseActorError {
    /// The kind is not one of the known actor kinds.
    #[error("unknown actor kind {0:?}")]
    UnknownKind(String),
    /// Analyst, Requester and API Key actors need an ID.
    #[error("actor kind {0} requires an id")]
    MissingId(ActorKind),
    /// Automation has no ID.
    #[error("actor kind automation must not have an id")]
    UnexpectedId,
}

impl Actor {
    /// The kind of this actor.
    #[must_use]
    pub const fn kind(&self) -> ActorKind {
        match self {
            Self::Analyst(_) => ActorKind::Analyst,
            Self::Requester(_) => ActorKind::Requester,
            Self::ApiKey(_) => ActorKind::ApiKey,
            Self::Automation => ActorKind::Automation,
        }
    }

    /// The actor's ID, or `None` for Automation.
    #[must_use]
    pub const fn id(&self) -> Option<Uuid> {
        match self {
            Self::Analyst(id) => Some(id.as_uuid()),
            Self::Requester(id) => Some(id.as_uuid()),
            Self::ApiKey(id) => Some(id.as_uuid()),
            Self::Automation => None,
        }
    }

    /// Rebuilds an actor from its stored `(actor_kind, actor_id)` columns.
    ///
    /// # Errors
    ///
    /// Returns [`ParseActorError`] when the kind is unknown or the ID does not match it.
    pub fn from_parts(kind: &str, id: Option<Uuid>) -> Result<Self, ParseActorError> {
        match (kind, id) {
            ("analyst", Some(id)) => Ok(Self::Analyst(id.into())),
            ("requester", Some(id)) => Ok(Self::Requester(id.into())),
            ("api_key", Some(id)) => Ok(Self::ApiKey(id.into())),
            ("automation", None) => Ok(Self::Automation),
            ("automation", Some(_)) => Err(ParseActorError::UnexpectedId),
            ("analyst", None) => Err(ParseActorError::MissingId(ActorKind::Analyst)),
            ("requester", None) => Err(ParseActorError::MissingId(ActorKind::Requester)),
            ("api_key", None) => Err(ParseActorError::MissingId(ActorKind::ApiKey)),
            (other, _) => Err(ParseActorError::UnknownKind(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_stored_parts() {
        let actors = [
            Actor::Analyst(AnalystId::new()),
            Actor::Requester(RequesterId::new()),
            Actor::ApiKey(ApiKeyId::new()),
            Actor::Automation,
        ];
        for actor in actors {
            let rebuilt = Actor::from_parts(actor.kind().as_str(), actor.id()).unwrap();
            assert_eq!(rebuilt, actor);
        }
    }

    #[test]
    fn rejects_inconsistent_parts() {
        assert_eq!(
            Actor::from_parts("analyst", None),
            Err(ParseActorError::MissingId(ActorKind::Analyst))
        );
        assert_eq!(
            Actor::from_parts("automation", Some(Uuid::nil())),
            Err(ParseActorError::UnexpectedId)
        );
        assert!(matches!(
            Actor::from_parts("agent", Some(Uuid::nil())),
            Err(ParseActorError::UnknownKind(_))
        ));
    }

    #[test]
    fn serializes_with_kind_tag() {
        let json = serde_json::to_value(Actor::Automation).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "automation" }));
    }
}
