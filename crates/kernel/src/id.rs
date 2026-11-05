//! Typed identifiers.
//!
//! Every aggregate gets its own ID newtype around a UUID v7 (time-ordered, generated in
//! the application). Use [`define_id!`](crate::define_id) to declare one:
//!
//! ```
//! cooper_kernel::define_id!(
//!     /// Identifies a Ticket.
//!     TicketId
//! );
//!
//! let id = TicketId::new();
//! let parsed: TicketId = id.to_string().parse().unwrap();
//! assert_eq!(id, parsed);
//! ```

use thiserror::Error;

/// A string could not be parsed as an identifier.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid {kind}: {value:?} is not a UUID")]
pub struct ParseIdError {
    /// Name of the ID type, for example `TicketId`.
    pub kind: &'static str,
    /// The rejected input.
    pub value: String,
}

/// Declares a typed identifier: a `Copy` newtype around [`uuid::Uuid`].
///
/// The generated type implements `new()` (UUID v7), `from_uuid`, `as_uuid`, `Display`,
/// `FromStr`, `From<Uuid>`, `Into<Uuid>`, ordering, hashing and transparent serde.
#[macro_export]
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            ::serde::Serialize, ::serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(::uuid::Uuid);

        impl $name {
            /// Generates a new time-ordered (UUID v7) identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(::uuid::Uuid::now_v7())
            }

            /// Wraps an existing UUID, for example one loaded from the database.
            #[must_use]
            pub const fn from_uuid(uuid: ::uuid::Uuid) -> Self {
                Self(uuid)
            }

            /// The underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> ::uuid::Uuid {
                self.0
            }
        }

        impl ::core::default::Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::core::str::FromStr for $name {
            type Err = $crate::id::ParseIdError;

            fn from_str(s: &str) -> ::core::result::Result<Self, Self::Err> {
                ::uuid::Uuid::parse_str(s).map(Self).map_err(|_| $crate::id::ParseIdError {
                    kind: stringify!($name),
                    value: s.to_owned(),
                })
            }
        }

        impl ::core::convert::From<::uuid::Uuid> for $name {
            fn from(uuid: ::uuid::Uuid) -> Self {
                Self(uuid)
            }
        }

        impl ::core::convert::From<$name> for ::uuid::Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

define_id!(
    /// Identifies a Company. Called **Company** in the UI and docs, **Tenant** in code.
    TenantId
);

define_id!(
    /// Identifies an Analyst. Shared because every context records Analysts as actors,
    /// assignees and owners.
    AnalystId
);

define_id!(
    /// Identifies a Requester.
    RequesterId
);

define_id!(
    /// Identifies an API Key.
    ApiKeyId
);

define_id!(
    /// Identifies a Team. Shared because tickets, tasks and checks are assigned to Teams
    /// (a Team is also a ticket Queue).
    TeamId
);

define_id!(
    /// Identifies a Department. Shared because tickets belong to a Department.
    DepartmentId
);

define_id!(
    /// Identifies a Ticket. Shared because other contexts link to tickets.
    TicketId
);

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn new_ids_are_uuid_v7_and_time_ordered() {
        let first = TenantId::new();
        let second = TenantId::new();
        assert_eq!(first.as_uuid().get_version_num(), 7);
        assert!(first < second);
    }

    #[test]
    fn parsing_rejects_non_uuid_with_type_name() {
        let err = "not-a-uuid".parse::<AnalystId>().unwrap_err();
        assert_eq!(err.kind, "AnalystId");
        assert_eq!(
            err.to_string(),
            "invalid AnalystId: \"not-a-uuid\" is not a UUID"
        );
    }

    #[test]
    fn serializes_as_plain_uuid_string() {
        let id = RequesterId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<RequesterId>(&json).unwrap(), id);
    }

    proptest! {
        #[test]
        fn display_and_parse_round_trip(bytes in any::<[u8; 16]>()) {
            let id = TenantId::from_uuid(uuid::Uuid::from_bytes(bytes));
            prop_assert_eq!(id.to_string().parse::<TenantId>().unwrap(), id);
        }
    }
}
