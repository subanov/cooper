//! Knowledge Articles attached to tasks as procedures (FR-TSK-007, ADR-0043). A procedure is
//! a copy of the article's text at the moment it is attached, so the task keeps the steps it
//! was given even when the article changes later.

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, TenantId};
use thiserror::Error;
use uuid::Uuid;

/// A procedure problem.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProcedureError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
}

fn invalid(field: &'static str, reason: impl Into<String>) -> ProcedureError {
    ProcedureError::Invalid { field, reason: reason.into() }
}

/// Longest procedure body, in characters.
pub const MAX_BODY_LEN: usize = 200_000;

/// The article as it reads now, as the knowledge context gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleSnapshot {
    /// The article.
    pub article_id: Uuid,
    /// Its Company (`None` for instance-wide articles).
    pub tenant_id: Option<TenantId>,
    /// Its current version.
    pub version: i32,
    /// Its title.
    pub title: String,
    /// Its sanitised body.
    pub body: String,
    /// Published and not archived.
    pub live: bool,
}

/// A procedure: an article's text, copied when it was attached to a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Procedure {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// The task.
    pub task_id: Uuid,
    /// The article it came from.
    pub article_id: Uuid,
    /// The article version it copies.
    pub article_version: i32,
    /// Title, as it was.
    pub title: String,
    /// Body, as it was.
    pub body: String,
    /// Who attached it.
    pub attached_by: Actor,
    /// When it was attached.
    pub attached_at: DateTime<Utc>,
}

impl Procedure {
    /// Copies a live article in the task's Company into a procedure.
    ///
    /// # Errors
    ///
    /// An article that is not published, belongs to another Company, or has an empty title
    /// or a body that is too long.
    pub fn attach(task: Uuid, tenant: TenantId, article: &ArticleSnapshot, by: Actor, now: DateTime<Utc>) -> Result<Self, ProcedureError> {
        if !article.live {
            return Err(invalid("article_id", "is not published"));
        }
        if article.tenant_id.is_some_and(|t| t != tenant) {
            return Err(invalid("article_id", "belongs to another Company"));
        }
        let title = article.title.trim().to_owned();
        if title.is_empty() || title.chars().count() > 500 {
            return Err(invalid("title", "must be 1 to 500 characters"));
        }
        if article.body.chars().count() > MAX_BODY_LEN {
            return Err(invalid("body", "is longer than 200000 characters"));
        }
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id: tenant,
            task_id: task,
            article_id: article.article_id,
            article_version: article.version,
            title,
            body: article.body.clone(),
            attached_by: by,
            attached_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test code

    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 12, 2, 9, 0, 0).unwrap()
    }

    fn article(tenant: Option<TenantId>) -> ArticleSnapshot {
        ArticleSnapshot {
            article_id: Uuid::now_v7(),
            tenant_id: tenant,
            version: 3,
            title: " Reset the VPN ".into(),
            body: "<ol><li>Open the client</li></ol>".into(),
            live: true,
        }
    }

    #[test]
    fn attaching_copies_the_article_as_it_reads_now() {
        let tenant = TenantId::new();
        let a = article(Some(tenant));
        let p = Procedure::attach(Uuid::now_v7(), tenant, &a, Actor::Automation, now()).unwrap();
        assert_eq!(p.title, "Reset the VPN");
        assert_eq!(p.body, a.body);
        assert_eq!(p.article_version, 3);
        assert_eq!(p.article_id, a.article_id);
    }

    #[test]
    fn a_later_change_to_the_article_does_not_change_the_procedure() {
        let tenant = TenantId::new();
        let mut a = article(None);
        let p = Procedure::attach(Uuid::now_v7(), tenant, &a, Actor::Automation, now()).unwrap();
        a.body = "<p>Changed</p>".into();
        a.version = 4;
        assert_ne!(p.body, a.body);
        assert_eq!(p.article_version, 3);
    }

    #[test]
    fn only_live_articles_of_the_same_company_can_be_attached() {
        let tenant = TenantId::new();
        let mut draft = article(None);
        draft.live = false;
        assert!(Procedure::attach(Uuid::now_v7(), tenant, &draft, Actor::Automation, now()).is_err());
        let other = article(Some(TenantId::new()));
        assert!(Procedure::attach(Uuid::now_v7(), tenant, &other, Actor::Automation, now()).is_err());
        assert!(Procedure::attach(Uuid::now_v7(), tenant, &article(None), Actor::Automation, now()).is_ok(), "instance-wide articles are shared");
    }

    #[test]
    fn titles_and_bodies_are_limited() {
        let tenant = TenantId::new();
        let mut blank = article(None);
        blank.title = "   ".into();
        assert!(Procedure::attach(Uuid::now_v7(), tenant, &blank, Actor::Automation, now()).is_err());
        let mut long = article(None);
        long.body = "x".repeat(MAX_BODY_LEN + 1);
        assert!(Procedure::attach(Uuid::now_v7(), tenant, &long, Actor::Automation, now()).is_err());
    }
}
