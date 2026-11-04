//! Cursor pagination (FR-GEN-003, AGENTS.md §6).
//!
//! Lists are paged with an opaque [`Cursor`] and a [`Limit`] of at most
//! [`Limit::MAX`] items. A cursor encodes the sort key of the last item returned
//! (for example `(created_at, id)`) so the next page can continue with a keyset query.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Number of items per page: between 1 and [`Limit::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct Limit(u32);

/// A requested page size is out of range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("limit must be between 1 and {max}, got {0}", max = Limit::MAX)]
pub struct LimitError(pub u32);

impl Limit {
    /// The largest page size the API allows.
    pub const MAX: u32 = 200;
    /// The page size used when the client does not ask for one.
    pub const DEFAULT: Self = Self(50);

    /// Validates a page size.
    ///
    /// # Errors
    ///
    /// Returns [`LimitError`] when `value` is 0 or above [`Limit::MAX`].
    pub const fn new(value: u32) -> Result<Self, LimitError> {
        if value == 0 || value > Self::MAX {
            Err(LimitError(value))
        } else {
            Ok(Self(value))
        }
    }

    /// The page size.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The page size plus one, for "fetch one extra row to know if there is a next page".
    #[must_use]
    pub const fn fetch_size(self) -> i64 {
        self.0 as i64 + 1
    }
}

impl Default for Limit {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<u32> for Limit {
    type Error = LimitError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Limit> for u32 {
    fn from(limit: Limit) -> Self {
        limit.0
    }
}

/// An opaque continuation token. Clients must not interpret it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(String);

/// A cursor string could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid cursor")]
pub struct CursorError;

impl Cursor {
    /// Encodes a sort key into a cursor.
    ///
    /// # Errors
    ///
    /// Fails only if `key` cannot be serialized.
    pub fn encode<K: Serialize>(key: &K) -> Result<Self, CursorError> {
        let json = serde_json::to_vec(key).map_err(|_| CursorError)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(json)))
    }

    /// Decodes the sort key from a cursor.
    ///
    /// # Errors
    ///
    /// Returns [`CursorError`] if the cursor was not produced by [`Cursor::encode`] for `K`.
    pub fn decode<K: DeserializeOwned>(&self) -> Result<K, CursorError> {
        let json = URL_SAFE_NO_PAD.decode(&self.0).map_err(|_| CursorError)?;
        serde_json::from_slice(&json).map_err(|_| CursorError)
    }

    /// Wraps a cursor string received from a client. Validation happens in
    /// [`Cursor::decode`].
    #[must_use]
    pub fn from_client(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The cursor as sent to clients.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One page of results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    /// The items on this page.
    pub items: Vec<T>,
    /// Cursor for the next page, or `None` on the last page.
    pub next_cursor: Option<Cursor>,
}

impl<T> Page<T> {
    /// Builds a page from up to `limit + 1` rows fetched in sort order. If the extra row
    /// is present it is dropped and `cursor_of` builds the cursor from the last kept item.
    ///
    /// # Errors
    ///
    /// Propagates errors from `cursor_of`.
    pub fn from_rows<F>(mut rows: Vec<T>, limit: Limit, cursor_of: F) -> Result<Self, CursorError>
    where
        F: FnOnce(&T) -> Result<Cursor, CursorError>,
    {
        let limit = limit.get() as usize;
        let next_cursor = if rows.len() > limit {
            rows.truncate(limit);
            rows.last().map(cursor_of).transpose()?
        } else {
            None
        };
        Ok(Self {
            items: rows,
            next_cursor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn limit_rejects_zero_and_above_max() {
        assert_eq!(Limit::new(0), Err(LimitError(0)));
        assert_eq!(Limit::new(201), Err(LimitError(201)));
        assert_eq!(Limit::new(200).map(Limit::get), Ok(200));
        assert_eq!(Limit::default().get(), 50);
    }

    #[test]
    fn limit_deserialization_validates() {
        assert!(serde_json::from_str::<Limit>("500").is_err());
        assert_eq!(serde_json::from_str::<Limit>("10").unwrap().get(), 10);
    }

    #[test]
    fn decoding_garbage_fails() {
        assert_eq!(
            Cursor::from_client("!!!").decode::<(i64, String)>(),
            Err(CursorError)
        );
        let wrong_shape = Cursor::encode(&"text").unwrap();
        assert_eq!(wrong_shape.decode::<(i64, i64)>(), Err(CursorError));
    }

    #[test]
    fn page_sets_cursor_only_when_more_rows_exist() {
        let limit = Limit::new(2).unwrap();
        let cursor_of = |n: &i32| Cursor::encode(n);

        let full = Page::from_rows(vec![1, 2, 3], limit, cursor_of).unwrap();
        assert_eq!(full.items, vec![1, 2]);
        assert_eq!(full.next_cursor.unwrap().decode::<i32>().unwrap(), 2);

        let last = Page::from_rows(vec![1, 2], limit, cursor_of).unwrap();
        assert_eq!(last.items, vec![1, 2]);
        assert!(last.next_cursor.is_none());
    }

    proptest! {
        #[test]
        fn cursor_round_trips(ts in any::<i64>(), id in "[a-f0-9-]{0,36}") {
            let cursor = Cursor::encode(&(ts, id.clone())).unwrap();
            prop_assert!(cursor.as_str().chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
            prop_assert_eq!(cursor.decode::<(i64, String)>().unwrap(), (ts, id));
        }

        #[test]
        fn limit_accepts_exactly_1_to_200(n in any::<u32>()) {
            prop_assert_eq!(Limit::new(n).is_ok(), (1..=200).contains(&n));
        }
    }
}
