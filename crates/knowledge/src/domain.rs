//! Knowledge domain (ADR-0028). Pure Rust: no I/O.
//!
//! Who may read an Article is answered here and only here (FR-KNW-004, -005): the
//! lifecycle, the Audience ladder, the Company and the access lists of the Article and
//! every Folder above it. Every axis narrows; nothing widens.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AnalystId, DomainEvent, RequesterId, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// A Knowledge rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KnowledgeError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Not allowed in the current state.
    #[error("{0}")]
    InvalidState(String),
}

impl KnowledgeError {
    /// A validation error.
    #[must_use]
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }

    fn state(reason: impl Into<String>) -> Self {
        Self::InvalidState(reason.into())
    }
}

/// Who an Article is for, in ascending reach (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    /// Analysts only (default).
    Internal,
    /// Also signed-in Requesters on the Portal.
    Customer,
    /// Anyone, including anonymous readers.
    Public,
}

impl Audience {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Customer => "customer",
            Self::Public => "public",
        }
    }
}

impl FromStr for Audience {
    type Err = KnowledgeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "internal" => Ok(Self::Internal),
            "customer" => Ok(Self::Customer),
            "public" => Ok(Self::Public),
            _ => Err(KnowledgeError::invalid(
                "audience",
                "must be internal, customer or public",
            )),
        }
    }
}

impl fmt::Display for Audience {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Someone an access list can name (FR-KNW-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AclPrincipal {
    /// An Analyst.
    Analyst(AnalystId),
    /// Every member of a Team.
    Team(TeamId),
    /// A Requester.
    Requester(RequesterId),
    /// Every current member of a User Group.
    UserGroup(Uuid),
}

impl AclPrincipal {
    /// Storage kind.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Analyst(_) => "analyst",
            Self::Team(_) => "team",
            Self::Requester(_) => "requester",
            Self::UserGroup(_) => "user_group",
        }
    }

    /// The named record's ID.
    #[must_use]
    pub const fn id(self) -> Uuid {
        match self {
            Self::Analyst(a) => a.as_uuid(),
            Self::Team(t) => t.as_uuid(),
            Self::Requester(r) => r.as_uuid(),
            Self::UserGroup(g) => g,
        }
    }

    /// Rebuilds a principal from storage.
    ///
    /// # Errors
    ///
    /// An unknown kind.
    pub fn from_parts(kind: &str, id: Uuid) -> Result<Self, KnowledgeError> {
        match kind {
            "analyst" => Ok(Self::Analyst(AnalystId::from_uuid(id))),
            "team" => Ok(Self::Team(TeamId::from_uuid(id))),
            "requester" => Ok(Self::Requester(RequesterId::from_uuid(id))),
            "user_group" => Ok(Self::UserGroup(id)),
            _ => Err(KnowledgeError::invalid(
                "principal_type",
                "must be analyst, team, requester or user_group",
            )),
        }
    }
}

/// The access rules of one Folder or Article.
///
/// An **Open** node admits everyone except the principals it names (rows are denials);
/// a **Restricted** node admits only the principals it names (rows are grants). The
/// polarity belongs to the node, so a grant and a denial can never contradict each
/// other. A node that inherits ignores its own rows. A Restricted node with no rows
/// admits nobody but the administrator floor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnowledgeAcl {
    /// Restricted (grants) rather than Open (denials).
    pub is_restricted: bool,
    /// Take the parent's rules instead of these.
    pub inherit: bool,
    /// The named principals.
    pub principals: BTreeSet<AclPrincipal>,
}

impl KnowledgeAcl {
    /// Inherits everything, names nobody: the default.
    #[must_use]
    pub fn inherited() -> Self {
        Self {
            is_restricted: false,
            inherit: true,
            principals: BTreeSet::new(),
        }
    }

    /// Whether this node, taken alone, admits someone holding `held`.
    #[must_use]
    pub fn permits(&self, held: &BTreeSet<AclPrincipal>) -> bool {
        if self.inherit {
            return true;
        }
        let named = self.principals.iter().any(|p| held.contains(p));
        if self.is_restricted { named } else { !named }
    }

    /// Checks the list size.
    ///
    /// # Errors
    ///
    /// More than 500 principals.
    pub fn validate(&self) -> Result<(), KnowledgeError> {
        if self.principals.len() > 500 {
            return Err(KnowledgeError::invalid("principals", "at most 500"));
        }
        Ok(())
    }
}

/// One Folder as the permission walk needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderNode {
    /// Parent, or none at the root.
    pub parent_id: Option<Uuid>,
    /// Its rules.
    pub access: KnowledgeAcl,
}

/// The Folder tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderTree {
    nodes: BTreeMap<Uuid, FolderNode>,
}

impl FolderTree {
    /// A tree of these nodes.
    #[must_use]
    pub const fn new(nodes: BTreeMap<Uuid, FolderNode>) -> Self {
        Self { nodes }
    }

    /// Whether someone holding `held` may reach `folder`: every Folder from it up to
    /// the root must admit them (a locked cabinet is locked). A cycle or a missing
    /// Folder fails closed.
    #[must_use]
    pub fn readable(&self, folder: Uuid, held: &BTreeSet<AclPrincipal>) -> bool {
        let mut seen = BTreeSet::new();
        let mut cursor = Some(folder);
        while let Some(id) = cursor {
            if !seen.insert(id) {
                return false;
            }
            let Some(node) = self.nodes.get(&id) else {
                return false;
            };
            if !node.access.permits(held) {
                return false;
            }
            cursor = node.parent_id;
        }
        true
    }

    /// Folders someone holding `held` may not reach.
    #[must_use]
    pub fn blocked(&self, held: &BTreeSet<AclPrincipal>) -> BTreeSet<Uuid> {
        self.nodes
            .keys()
            .copied()
            .filter(|id| !self.readable(*id, held))
            .collect()
    }

    /// Whether moving `folder` under `new_parent` keeps the tree a tree.
    #[must_use]
    pub fn would_cycle(&self, folder: Uuid, new_parent: Uuid) -> bool {
        let mut seen = BTreeSet::new();
        let mut cursor = Some(new_parent);
        while let Some(id) = cursor {
            if id == folder || !seen.insert(id) {
                return true;
            }
            cursor = self.nodes.get(&id).and_then(|n| n.parent_id);
        }
        false
    }

    /// Whether a Folder exists.
    #[must_use]
    pub fn contains(&self, folder: Uuid) -> bool {
        self.nodes.contains_key(&folder)
    }
}

/// Who is reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    /// What access lists can name them by: themselves, their Teams, their User Groups.
    pub held: BTreeSet<AclPrincipal>,
    /// The widest Audience they may read below: Analysts read every Audience,
    /// Requesters `customer` and `public`.
    pub lowest_audience: Audience,
    /// The administrator floor: access lists do not apply (audited when used).
    pub floor: bool,
}

impl Viewer {
    /// Whether the Audience lets them in.
    #[must_use]
    pub fn reaches(&self, audience: Audience) -> bool {
        audience >= self.lowest_audience
    }
}

/// Whether the access lists admit `viewer` to an Article in `folder` with `access`, and
/// whether only the administrator floor did.
#[must_use]
pub fn acl_decision(
    tree: &FolderTree,
    folder: Option<Uuid>,
    access: &KnowledgeAcl,
    viewer: &Viewer,
) -> AclDecision {
    let own = access.permits(&viewer.held);
    let container = folder.is_none_or(|f| tree.readable(f, &viewer.held));
    match (own && container, viewer.floor) {
        (true, _) => AclDecision::Granted,
        (false, true) => AclDecision::Floor,
        (false, false) => AclDecision::Denied,
    }
}

/// The outcome of the access lists for one reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclDecision {
    /// The lists admit them.
    Granted,
    /// Only the administrator floor admits them (audit it).
    Floor,
    /// Not readable.
    Denied,
}

// ----- HTML -------------------------------------------------------------------------------

/// Sanitises Article HTML (FR-KNW-001): formatting, links, images, tables and code
/// survive; scripts, styles, event handlers, forms and unsafe URLs do not. Links get
/// `rel="noopener noreferrer"`.
#[must_use]
pub fn sanitise_html(html: &str) -> String {
    ammonia::Builder::default()
        .add_tags(["h1", "h2", "h5", "h6", "figure", "figcaption", "mark"])
        .link_rel(Some("noopener noreferrer"))
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .clean(html)
        .to_string()
}

/// The plain text of sanitised HTML: blocks become lines, entities are decoded. Used for
/// search and for inserting an Article into a reply (FR-KNW-008).
#[must_use]
pub fn html_to_text(html: &str) -> String {
    let mut marked = html.to_owned();
    for tag in [
        "</p>",
        "<br>",
        "<br />",
        "</li>",
        "</h1>",
        "</h2>",
        "</h3>",
        "</h4>",
        "</h5>",
        "</h6>",
        "</div>",
        "</tr>",
        "</pre>",
        "</blockquote>",
    ] {
        marked = marked.replace(tag, &format!("{tag}\n"));
    }
    let stripped = ammonia::Builder::empty().clean(&marked).to_string();
    let decoded = stripped
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    let lines: Vec<&str> = decoded.lines().map(str::trim).collect();
    let mut out = String::new();
    let mut blank = false;
    for line in lines {
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        out.push_str(line);
    }
    out
}

/// Normalises tags: trimmed, lower-case, unique, 1–50 characters, at most 30.
///
/// # Errors
///
/// A tag that is too long, or too many.
pub fn clean_tags(tags: &[String]) -> Result<Vec<String>, KnowledgeError> {
    let mut out = BTreeSet::new();
    for t in tags {
        let t = t.trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.chars().count() > 50 {
            return Err(KnowledgeError::invalid(
                "tags",
                "each tag is at most 50 characters",
            ));
        }
        out.insert(t);
    }
    if out.len() > 30 {
        return Err(KnowledgeError::invalid("tags", "at most 30"));
    }
    Ok(out.into_iter().collect())
}

fn clean_title(title: &str) -> Result<String, KnowledgeError> {
    let t = title.trim();
    if t.is_empty() || t.chars().count() > 255 {
        return Err(KnowledgeError::invalid(
            "title",
            "must be 1 to 255 characters",
        ));
    }
    Ok(t.to_owned())
}

// ----- Folders ----------------------------------------------------------------------------

/// A Knowledge Folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeFolder {
    /// ID.
    pub id: Uuid,
    /// Company, or none when shared with every Company.
    pub tenant_id: Option<TenantId>,
    /// Parent, or none at the root.
    pub parent_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Who to ask about it.
    pub owner_id: Option<AnalystId>,
    /// Its rules.
    pub access: KnowledgeAcl,
    /// Who created it.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl KnowledgeFolder {
    /// Validates the name.
    ///
    /// # Errors
    ///
    /// An empty or long name.
    pub fn validate(&mut self) -> Result<(), KnowledgeError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 255 {
            return Err(KnowledgeError::invalid(
                "name",
                "must be 1 to 255 characters",
            ));
        }
        self.name = name.to_owned();
        self.access.validate()
    }
}

/// Checks that an Article or child Folder may live in `parent`: a Company Folder holds
/// only that Company's content; a shared Folder holds anything.
///
/// # Errors
///
/// A Company mismatch.
pub fn check_placement(
    content: Option<TenantId>,
    parent: Option<TenantId>,
) -> Result<(), KnowledgeError> {
    match (parent, content) {
        (None, _) => Ok(()),
        (Some(p), Some(c)) if p == c => Ok(()),
        _ => Err(KnowledgeError::invalid(
            "folder_id",
            "a Company KnowledgeFolder holds only that Company's content",
        )),
    }
}

// ----- Articles ---------------------------------------------------------------------------

/// What an Analyst edits on an Article.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArticleDraft {
    /// Title.
    pub title: String,
    /// HTML body (sanitised on save).
    pub body: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Folder, or the root.
    pub folder_id: Option<Uuid>,
    /// Owner.
    pub owner_id: Option<AnalystId>,
    /// Audience.
    pub audience: Option<Audience>,
    /// Review Date.
    pub next_review_date: Option<NaiveDate>,
}

/// A Knowledge Article (FR-KNW-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeArticle {
    id: Uuid,
    tenant_id: Option<TenantId>,
    folder_id: Option<Uuid>,
    title: String,
    body: String,
    body_text: String,
    tags: Vec<String>,
    audience: Audience,
    author_id: Option<AnalystId>,
    owner_id: Option<AnalystId>,
    is_published: bool,
    is_archived: bool,
    archived_at: Option<DateTime<Utc>>,
    access: KnowledgeAcl,
    next_review_date: Option<NaiveDate>,
    version: i32,
    view_count: i64,
    source_problem_id: Option<Uuid>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// An immutable saved state of an Article (FR-KNW-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeArticleVersion {
    /// Version number.
    pub version_no: i32,
    /// Title then.
    pub title: String,
    /// Body then (sanitised).
    pub body: String,
    /// Who saved it.
    pub saved_by: Actor,
    /// When.
    pub saved_at: DateTime<Utc>,
}

/// Stored state of an Article, for [`KnowledgeArticle::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleRecord {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: Option<TenantId>,
    /// Folder.
    pub folder_id: Option<Uuid>,
    /// Title.
    pub title: String,
    /// Sanitised body.
    pub body: String,
    /// Plain text.
    pub body_text: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Audience.
    pub audience: Audience,
    /// Author.
    pub author_id: Option<AnalystId>,
    /// Owner.
    pub owner_id: Option<AnalystId>,
    /// Published.
    pub is_published: bool,
    /// Archived.
    pub is_archived: bool,
    /// When archived.
    pub archived_at: Option<DateTime<Utc>>,
    /// Rules.
    pub access: KnowledgeAcl,
    /// Review Date.
    pub next_review_date: Option<NaiveDate>,
    /// Current version.
    pub version: i32,
    /// Views.
    pub view_count: i64,
    /// Known Error source.
    pub source_problem_id: Option<Uuid>,
    /// Creator.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl KnowledgeArticle {
    /// A new, unpublished Article at version 1.
    ///
    /// # Errors
    ///
    /// Invalid title or tags.
    pub fn create(
        tenant_id: Option<TenantId>,
        draft: ArticleDraft,
        source_problem_id: Option<Uuid>,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<(Self, KnowledgeArticleVersion), KnowledgeError> {
        let author_id = match by {
            Actor::Analyst(a) => Some(a),
            _ => None,
        };
        let mut a = Self {
            id: Uuid::now_v7(),
            tenant_id,
            folder_id: None,
            title: String::new(),
            body: String::new(),
            body_text: String::new(),
            tags: Vec::new(),
            audience: Audience::Internal,
            author_id,
            owner_id: draft.owner_id.or(author_id),
            is_published: false,
            is_archived: false,
            archived_at: None,
            access: KnowledgeAcl::inherited(),
            next_review_date: None,
            version: 0,
            view_count: 0,
            source_problem_id,
            created_by: by,
            created_at: now,
            updated_at: now,
        };
        let v = a.edit(draft, by, now)?.ok_or_else(|| {
            KnowledgeError::state("a new KnowledgeArticle always has a first version")
        })?;
        Ok((a, v))
    }

    /// Rebuilds a stored Article.
    #[must_use]
    pub fn restore(r: ArticleRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            folder_id: r.folder_id,
            title: r.title,
            body: r.body,
            body_text: r.body_text,
            tags: r.tags,
            audience: r.audience,
            author_id: r.author_id,
            owner_id: r.owner_id,
            is_published: r.is_published,
            is_archived: r.is_archived,
            archived_at: r.archived_at,
            access: r.access,
            next_review_date: r.next_review_date,
            version: r.version,
            view_count: r.view_count,
            source_problem_id: r.source_problem_id,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    /// Saves an edit. A new title or body makes a new immutable version, returned for
    /// storage; other fields (tags, folder, owner, audience, Review Date) change in place.
    ///
    /// # Errors
    ///
    /// Invalid title or tags, or an archived Article.
    pub fn edit(
        &mut self,
        draft: ArticleDraft,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Option<KnowledgeArticleVersion>, KnowledgeError> {
        if self.is_archived {
            return Err(KnowledgeError::state(
                "restore the KnowledgeArticle before editing it",
            ));
        }
        let title = clean_title(&draft.title)?;
        if draft.body.len() > 2_000_000 {
            return Err(KnowledgeError::invalid("body", "at most 2 MB"));
        }
        let body = sanitise_html(&draft.body);
        self.tags = clean_tags(&draft.tags)?;
        self.folder_id = draft.folder_id;
        if draft.owner_id.is_some() {
            self.owner_id = draft.owner_id;
        }
        self.audience = draft.audience.unwrap_or(self.audience);
        self.next_review_date = draft.next_review_date;
        self.updated_at = now;
        if self.version > 0 && title == self.title && body == self.body {
            return Ok(None);
        }
        self.version += 1;
        self.body_text = html_to_text(&body);
        self.title = title;
        self.body = body;
        Ok(Some(KnowledgeArticleVersion {
            version_no: self.version,
            title: self.title.clone(),
            body: self.body.clone(),
            saved_by: by,
            saved_at: now,
        }))
    }

    /// Restores an earlier version as a new version (versions are never changed).
    ///
    /// # Errors
    ///
    /// An archived Article.
    pub fn revert_to(
        &mut self,
        old: &KnowledgeArticleVersion,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Option<KnowledgeArticleVersion>, KnowledgeError> {
        let draft = ArticleDraft {
            title: old.title.clone(),
            body: old.body.clone(),
            tags: self.tags.clone(),
            folder_id: self.folder_id,
            owner_id: self.owner_id,
            audience: Some(self.audience),
            next_review_date: self.next_review_date,
        };
        self.edit(draft, by, now)
    }

    /// Publishes or unpublishes (FR-KNW-003).
    ///
    /// # Errors
    ///
    /// Publishing an archived Article.
    pub fn set_published(
        &mut self,
        published: bool,
        now: DateTime<Utc>,
    ) -> Result<bool, KnowledgeError> {
        if published && self.is_archived {
            return Err(KnowledgeError::state(
                "an archived KnowledgeArticle cannot be published",
            ));
        }
        if self.is_published == published {
            return Ok(false);
        }
        self.is_published = published;
        self.updated_at = now;
        Ok(true)
    }

    /// Archives (the recycle bin): hidden from every reader, kept with its history.
    /// Archiving also unpublishes, so a restored Article is never live by surprise.
    ///
    /// # Errors
    ///
    /// Already archived.
    pub fn archive(&mut self, now: DateTime<Utc>) -> Result<(), KnowledgeError> {
        if self.is_archived {
            return Err(KnowledgeError::state(
                "the KnowledgeArticle is already archived",
            ));
        }
        self.is_archived = true;
        self.is_published = false;
        self.archived_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    /// Restores from the archive, unpublished.
    ///
    /// # Errors
    ///
    /// Not archived.
    pub fn unarchive(&mut self, now: DateTime<Utc>) -> Result<(), KnowledgeError> {
        if !self.is_archived {
            return Err(KnowledgeError::state(
                "the KnowledgeArticle is not archived",
            ));
        }
        self.is_archived = false;
        self.archived_at = None;
        self.updated_at = now;
        Ok(())
    }

    /// Replaces the access rules.
    ///
    /// # Errors
    ///
    /// Too many principals.
    pub fn set_access(
        &mut self,
        access: KnowledgeAcl,
        now: DateTime<Utc>,
    ) -> Result<(), KnowledgeError> {
        access.validate()?;
        self.access = access;
        self.updated_at = now;
        Ok(())
    }

    /// Whether the Article is live: published and not archived.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.is_published && !self.is_archived
    }

    /// Whether the Review Date has come (FR-KNW-007).
    #[must_use]
    pub fn review_due(&self, today: NaiveDate) -> bool {
        !self.is_archived && self.next_review_date.is_some_and(|d| d <= today)
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Company, or none when shared.
    #[must_use]
    pub const fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
    /// Folder.
    #[must_use]
    pub const fn folder_id(&self) -> Option<Uuid> {
        self.folder_id
    }
    /// Title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Sanitised HTML body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
    /// Plain text body.
    #[must_use]
    pub fn body_text(&self) -> &str {
        &self.body_text
    }
    /// Tags.
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }
    /// Audience.
    #[must_use]
    pub const fn audience(&self) -> Audience {
        self.audience
    }
    /// Author.
    #[must_use]
    pub const fn author_id(&self) -> Option<AnalystId> {
        self.author_id
    }
    /// Owner.
    #[must_use]
    pub const fn owner_id(&self) -> Option<AnalystId> {
        self.owner_id
    }
    /// Published.
    #[must_use]
    pub const fn is_published(&self) -> bool {
        self.is_published
    }
    /// Archived.
    #[must_use]
    pub const fn is_archived(&self) -> bool {
        self.is_archived
    }
    /// When archived.
    #[must_use]
    pub const fn archived_at(&self) -> Option<DateTime<Utc>> {
        self.archived_at
    }
    /// Access rules.
    #[must_use]
    pub const fn access(&self) -> &KnowledgeAcl {
        &self.access
    }
    /// Review Date.
    #[must_use]
    pub const fn next_review_date(&self) -> Option<NaiveDate> {
        self.next_review_date
    }
    /// Current version.
    #[must_use]
    pub const fn version(&self) -> i32 {
        self.version
    }
    /// Views.
    #[must_use]
    pub const fn view_count(&self) -> i64 {
        self.view_count
    }
    /// The Known Error it was published from.
    #[must_use]
    pub const fn source_problem_id(&self) -> Option<Uuid> {
        self.source_problem_id
    }
    /// Creator.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Audited fields (the body by version number, not by content).
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, serde_json::Value)> {
        use serde_json::json;
        vec![
            ("title", json!(self.title)),
            ("version", json!(self.version)),
            ("folder_id", json!(self.folder_id)),
            ("tags", json!(self.tags)),
            ("audience", json!(self.audience.as_str())),
            ("owner_id", json!(self.owner_id.map(|a| a.as_uuid()))),
            ("is_published", json!(self.is_published)),
            ("is_archived", json!(self.is_archived)),
            ("next_review_date", json!(self.next_review_date)),
        ]
    }
}

/// An Article's Review Date has come (FR-KNW-007).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArticleReviewDue {
    /// The Article.
    pub article_id: Uuid,
    /// Its Company, if any.
    pub tenant_id: Option<TenantId>,
    /// Its Owner.
    pub owner_id: AnalystId,
    /// The Review Date.
    pub review_date: NaiveDate,
}

impl DomainEvent for ArticleReviewDue {
    const EVENT_TYPE: &'static str = "knowledge_article.review_due";
    const AGGREGATE_TYPE: &'static str = "knowledge_article";
    fn aggregate_id(&self) -> Uuid {
        self.article_id
    }
    fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-07T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn analyst() -> AnalystId {
        AnalystId::new()
    }

    fn restricted(to: &[AclPrincipal]) -> KnowledgeAcl {
        KnowledgeAcl {
            is_restricted: true,
            inherit: false,
            principals: to.iter().copied().collect(),
        }
    }

    fn open_denying(who: &[AclPrincipal]) -> KnowledgeAcl {
        KnowledgeAcl {
            is_restricted: false,
            inherit: false,
            principals: who.iter().copied().collect(),
        }
    }

    fn viewer(held: &[AclPrincipal]) -> Viewer {
        Viewer {
            held: held.iter().copied().collect(),
            lowest_audience: Audience::Internal,
            floor: false,
        }
    }

    #[test]
    fn open_nodes_deny_the_named_and_restricted_nodes_admit_only_them() {
        let (a, b) = (
            AclPrincipal::Analyst(analyst()),
            AclPrincipal::Analyst(analyst()),
        );
        let held_a: BTreeSet<_> = [a].into();
        let held_b: BTreeSet<_> = [b].into();
        assert!(restricted(&[a]).permits(&held_a));
        assert!(!restricted(&[a]).permits(&held_b));
        assert!(!restricted(&[]).permits(&held_a), "restricted to nobody");
        assert!(!open_denying(&[a]).permits(&held_a));
        assert!(open_denying(&[a]).permits(&held_b));
        let mut inheriting = restricted(&[]);
        inheriting.inherit = true;
        assert!(
            inheriting.permits(&held_a),
            "an inheriting node has no say of its own"
        );
    }

    #[test]
    fn every_folder_above_must_admit_the_reader() {
        let team = AclPrincipal::Team(TeamId::new());
        let (root, child, grandchild) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let tree = FolderTree::new(BTreeMap::from([
            (
                root,
                FolderNode {
                    parent_id: None,
                    access: restricted(&[team]),
                },
            ),
            (
                child,
                FolderNode {
                    parent_id: Some(root),
                    access: KnowledgeAcl::inherited(),
                },
            ),
            (
                grandchild,
                FolderNode {
                    parent_id: Some(child),
                    access: KnowledgeAcl::inherited(),
                },
            ),
        ]));
        let member = viewer(&[team]);
        let outsider = viewer(&[AclPrincipal::Analyst(analyst())]);
        assert!(tree.readable(grandchild, &member.held));
        assert!(
            !tree.readable(grandchild, &outsider.held),
            "inherits the restriction"
        );
        assert_eq!(tree.blocked(&outsider.held).len(), 3);
        assert_eq!(
            acl_decision(
                &tree,
                Some(grandchild),
                &KnowledgeAcl::inherited(),
                &outsider
            ),
            AclDecision::Denied
        );
        let admin = Viewer {
            floor: true,
            ..outsider
        };
        assert_eq!(
            acl_decision(&tree, Some(grandchild), &KnowledgeAcl::inherited(), &admin),
            AclDecision::Floor
        );
        assert_eq!(
            acl_decision(&tree, Some(grandchild), &KnowledgeAcl::inherited(), &member),
            AclDecision::Granted
        );
        assert_eq!(
            acl_decision(&tree, Some(grandchild), &open_denying(&[team]), &member),
            AclDecision::Denied,
            "the Article's own rules narrow further"
        );
        // An Article's own grant cannot reach past a locked Folder.
        let grant_outsider = restricted(&outsider_principal(&admin));
        assert_eq!(
            acl_decision(
                &tree,
                Some(child),
                &grant_outsider,
                &Viewer {
                    floor: false,
                    ..admin
                }
            ),
            AclDecision::Denied
        );
    }

    fn outsider_principal(v: &Viewer) -> Vec<AclPrincipal> {
        v.held.iter().copied().collect()
    }

    #[test]
    fn cycles_fail_closed_and_moves_cannot_create_them() {
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        let tree = FolderTree::new(BTreeMap::from([
            (
                a,
                FolderNode {
                    parent_id: Some(b),
                    access: KnowledgeAcl::inherited(),
                },
            ),
            (
                b,
                FolderNode {
                    parent_id: Some(a),
                    access: KnowledgeAcl::inherited(),
                },
            ),
        ]));
        assert!(!tree.readable(a, &BTreeSet::new()));
        let (root, child) = (Uuid::now_v7(), Uuid::now_v7());
        let tree = FolderTree::new(BTreeMap::from([
            (
                root,
                FolderNode {
                    parent_id: None,
                    access: KnowledgeAcl::inherited(),
                },
            ),
            (
                child,
                FolderNode {
                    parent_id: Some(root),
                    access: KnowledgeAcl::inherited(),
                },
            ),
        ]));
        assert!(tree.would_cycle(root, child));
        assert!(tree.would_cycle(root, root));
        assert!(!tree.would_cycle(child, root));
    }

    #[test]
    fn the_audience_ladder_narrows_requesters() {
        let requester = Viewer {
            held: BTreeSet::new(),
            lowest_audience: Audience::Customer,
            floor: false,
        };
        assert!(!requester.reaches(Audience::Internal));
        assert!(requester.reaches(Audience::Customer) && requester.reaches(Audience::Public));
        assert!(viewer(&[]).reaches(Audience::Internal));
        assert_eq!("customer".parse::<Audience>(), Ok(Audience::Customer));
    }

    #[test]
    fn html_is_sanitised_and_flattened_to_text() {
        let dirty = r#"<h2>Reset</h2><p onclick="x()">Open <a href="javascript:alert(1)">this</a> &amp; <b>that</b></p><script>alert(1)</script><ul><li>One</li><li>Two</li></ul>"#;
        let clean = sanitise_html(dirty);
        assert!(
            !clean.contains("script")
                && !clean.contains("onclick")
                && !clean.contains("javascript"),
            "{clean}"
        );
        assert!(clean.contains("<b>that</b>"));
        assert_eq!(html_to_text(&clean), "Reset\nOpen this & that\nOne\nTwo");
        let link = sanitise_html(r#"<a href="https://example.com">x</a>"#);
        assert!(link.contains("noopener"), "{link}");
    }

    #[test]
    fn saves_make_versions_only_when_content_changes() {
        let me = Actor::Analyst(analyst());
        let draft = ArticleDraft {
            title: " VPN setup ".into(),
            body: "<p>Step 1</p>".into(),
            tags: vec!["VPN".into(), "vpn ".into()],
            ..ArticleDraft::default()
        };
        let (mut a, v1) = KnowledgeArticle::create(None, draft.clone(), None, me, now()).unwrap();
        assert_eq!((a.version(), v1.version_no), (1, 1));
        assert_eq!(a.title(), "VPN setup");
        assert_eq!(a.tags(), ["vpn"]);
        assert!(!a.is_published(), "new Articles start unpublished");
        assert_eq!(
            a.owner_id(),
            a.author_id(),
            "the author owns it unless told otherwise"
        );
        let same = a
            .edit(
                ArticleDraft {
                    tags: vec!["remote".into()],
                    ..draft.clone()
                },
                me,
                now(),
            )
            .unwrap();
        assert!(same.is_none(), "tags alone make no version");
        let v2 = a
            .edit(
                ArticleDraft {
                    body: "<p>Step 1</p><p>Step 2</p>".into(),
                    ..draft.clone()
                },
                me,
                now(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(v2.version_no, 2);
        let v3 = a.revert_to(&v1, me, now()).unwrap().unwrap();
        assert_eq!(
            (v3.version_no, v3.body.as_str()),
            (3, "<p>Step 1</p>"),
            "reverting adds a version"
        );
        assert!(
            KnowledgeArticle::create(None, ArticleDraft::default(), None, me, now()).is_err(),
            "a title is required"
        );
    }

    #[test]
    fn archiving_unpublishes_and_blocks_edits_until_restored() {
        let me = Actor::Analyst(analyst());
        let (mut a, _) = KnowledgeArticle::create(
            None,
            ArticleDraft {
                title: "T".into(),
                ..ArticleDraft::default()
            },
            None,
            me,
            now(),
        )
        .unwrap();
        assert!(a.set_published(true, now()).unwrap());
        assert!(a.is_live());
        a.archive(now()).unwrap();
        assert!(!a.is_live() && !a.is_published());
        assert!(a.set_published(true, now()).is_err());
        assert!(
            a.edit(
                ArticleDraft {
                    title: "U".into(),
                    ..ArticleDraft::default()
                },
                me,
                now()
            )
            .is_err()
        );
        a.unarchive(now()).unwrap();
        assert!(!a.is_published(), "restored unpublished");
        assert!(a.unarchive(now()).is_err());
    }

    #[test]
    fn review_dates_come_due_and_placement_respects_companies() {
        let me = Actor::Analyst(analyst());
        let today = now().date_naive();
        let draft = ArticleDraft {
            title: "T".into(),
            next_review_date: Some(today),
            ..ArticleDraft::default()
        };
        let (a, _) = KnowledgeArticle::create(None, draft, None, me, now()).unwrap();
        assert!(a.review_due(today));
        assert!(!a.review_due(today.pred_opt().unwrap()));
        let (x, y) = (TenantId::new(), TenantId::new());
        assert!(check_placement(Some(x), None).is_ok());
        assert!(check_placement(None, None).is_ok());
        assert!(check_placement(Some(x), Some(x)).is_ok());
        assert!(check_placement(Some(y), Some(x)).is_err());
        assert!(
            check_placement(None, Some(x)).is_err(),
            "a shared Article cannot hide in a Company Folder"
        );
        assert_eq!(
            clean_tags(&["a".repeat(51)]).unwrap_err(),
            KnowledgeError::invalid("tags", "each tag is at most 50 characters")
        );
    }
}
