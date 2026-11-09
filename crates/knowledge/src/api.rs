//! HTTP API of Knowledge (FR-KNW, FR-PRT-005).

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{AnalystId, Cursor, Limit, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, ArticleListInput, FolderInput, KnowledgeService};
use crate::domain::{
    AclPrincipal, ArticleDraft, Audience, KnowledgeAcl, KnowledgeArticle, KnowledgeArticleVersion,
    KnowledgeError, KnowledgeFolder,
};
use crate::ports::{HistoryEntry, Lifecycle, TicketLink};

/// State the handlers need.
#[derive(Clone)]
pub struct KnowledgeApi {
    /// The service.
    pub service: KnowledgeService,
}

/// `/api/v1` path prefixes owned by the Knowledge Module (FR-GEN-005). The Portal's
/// Knowledge pages belong to it too (the longest prefix wins).
pub const ROUTE_PREFIXES: &[&str] = &["/knowledge", "/portal/knowledge"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    KnowledgeApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_knowledge_folders, create_knowledge_folder))
        .routes(routes!(update_knowledge_folder, delete_knowledge_folder))
        .routes(routes!(set_knowledge_folder_access))
        .routes(routes!(list_knowledge_articles, create_knowledge_article))
        .routes(routes!(get_knowledge_article, edit_knowledge_article))
        .routes(routes!(set_knowledge_article_published))
        .routes(routes!(archive_knowledge_article))
        .routes(routes!(unarchive_knowledge_article))
        .routes(routes!(list_knowledge_article_versions))
        .routes(routes!(revert_knowledge_article))
        .routes(routes!(set_knowledge_article_access))
        .routes(routes!(knowledge_article_history))
        .routes(routes!(
            list_knowledge_article_tickets,
            link_knowledge_article_ticket
        ))
        .routes(routes!(unlink_knowledge_article_ticket))
        .routes(routes!(search_portal_knowledge))
        .routes(routes!(read_portal_knowledge_article))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<KnowledgeError> for ApiError {
    fn from(e: KnowledgeError) -> Self {
        Self(AppError::Rule(e))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let detail = self.0.to_string();
        let typed = |status: StatusCode, slug: &str, title: &str| {
            ProblemJson::new(status)
                .with_type(format!("https://cooper.dev/problems/{slug}"), title)
                .with_detail(detail.clone())
        };
        match self.0 {
            AppError::Rule(KnowledgeError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(KnowledgeError::InvalidState(_)) => typed(
                StatusCode::CONFLICT,
                "rule-violated",
                "Not allowed in the current state",
            ),
            AppError::Forbidden(f) => forbidden(&f),
            AppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            AppError::Conflict(_) => typed(StatusCode::CONFLICT, "conflict", "Conflict"),
            AppError::Internal => ProblemJson::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn created<T: Serialize>(body: T) -> Response {
    (StatusCode::CREATED, Json(body)).into_response()
}

fn limit(v: Option<u32>) -> Result<Limit, ApiError> {
    v.map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| KnowledgeError::invalid("limit", "must be between 1 and 200").into())
}

// ----- KnowledgeAcl lists -------------------------------------------------------------------------

/// Someone an access list names.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgePrincipalDto {
    /// `analyst`, `team`, `requester` or `user_group`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Their ID.
    pub id: Uuid,
}

/// Access rules of a Folder or Article.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeAccessDto {
    /// Restricted: only the named may read. Open (false): everyone but the named.
    pub is_restricted: bool,
    /// Take the parent Folder's rules and ignore these.
    pub inherit: bool,
    /// The named.
    #[serde(default)]
    pub principals: Vec<KnowledgePrincipalDto>,
}

impl From<&KnowledgeAcl> for KnowledgeAccessDto {
    fn from(a: &KnowledgeAcl) -> Self {
        Self {
            is_restricted: a.is_restricted,
            inherit: a.inherit,
            principals: a
                .principals
                .iter()
                .map(|p| KnowledgePrincipalDto {
                    kind: p.kind().to_owned(),
                    id: p.id(),
                })
                .collect(),
        }
    }
}

impl KnowledgeAccessDto {
    fn access(self) -> Result<KnowledgeAcl, ApiError> {
        let principals = self
            .principals
            .into_iter()
            .map(|p| AclPrincipal::from_parts(&p.kind, p.id))
            .collect::<Result<BTreeSet<_>, _>>()?;
        Ok(KnowledgeAcl {
            is_restricted: self.is_restricted,
            inherit: self.inherit,
            principals,
        })
    }
}

// ----- Folders ------------------------------------------------------------------------------

/// A Knowledge Folder.
#[derive(Debug, Serialize, ToSchema)]
pub struct KnowledgeFolderDto {
    /// ID.
    pub id: Uuid,
    /// Company, or null when shared with every Company.
    pub company_id: Option<Uuid>,
    /// Parent, or null at the root.
    pub parent_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Owner.
    pub owner_id: Option<Uuid>,
    /// Access rules.
    pub access: KnowledgeAccessDto,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<KnowledgeFolder> for KnowledgeFolderDto {
    fn from(f: KnowledgeFolder) -> Self {
        Self {
            id: f.id,
            company_id: f.tenant_id.map(|t| t.as_uuid()),
            parent_id: f.parent_id,
            name: f.name,
            owner_id: f.owner_id.map(|a| a.as_uuid()),
            access: (&f.access).into(),
            updated_at: f.updated_at,
        }
    }
}

/// Creates or changes a Folder.
#[derive(Debug, Deserialize, ToSchema)]
pub struct KnowledgeFolderRequest {
    /// Company (create only); null shares it with every Company.
    pub company_id: Option<Uuid>,
    /// Parent, or null for the root.
    pub parent_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Owner.
    pub owner_id: Option<Uuid>,
}

impl From<KnowledgeFolderRequest> for FolderInput {
    fn from(r: KnowledgeFolderRequest) -> Self {
        Self {
            tenant_id: r.company_id.map(TenantId::from_uuid),
            parent_id: r.parent_id,
            name: r.name,
            owner_id: r.owner_id.map(AnalystId::from_uuid),
        }
    }
}

/// Folders you may reach (needs `knowledge.read`).
#[utoipa::path(get, path = "/knowledge/folders", tag = "Knowledge", responses((status = 200, body = Vec<KnowledgeFolderDto>)))]
async fn list_knowledge_folders(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<KnowledgeFolderDto>>> {
    Ok(Json(
        api.service
            .folders(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Creates a Folder (needs `knowledge.manage`).
#[utoipa::path(post, path = "/knowledge/folders", tag = "Knowledge", request_body = KnowledgeFolderRequest, responses((status = 201, body = KnowledgeFolderDto)))]
async fn create_knowledge_folder(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<KnowledgeFolderRequest>,
) -> ApiResult<Response> {
    Ok(created(KnowledgeFolderDto::from(
        api.service.create_folder(&p, b.into()).await?,
    )))
}

/// Renames, moves or re-owns a Folder (needs `knowledge.manage`).
#[utoipa::path(put, path = "/knowledge/folders/{id}", tag = "Knowledge", params(("id" = Uuid, Path)), request_body = KnowledgeFolderRequest, responses((status = 200, body = KnowledgeFolderDto)))]
async fn update_knowledge_folder(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<KnowledgeFolderRequest>,
) -> ApiResult<Json<KnowledgeFolderDto>> {
    Ok(Json(
        api.service.update_folder(&p, id, b.into()).await?.into(),
    ))
}

/// Deletes an empty Folder (needs `knowledge.manage`).
#[utoipa::path(delete, path = "/knowledge/folders/{id}", tag = "Knowledge", params(("id" = Uuid, Path)), responses((status = 204)))]
async fn delete_knowledge_folder(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api.service.delete_folder(&p, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces a Folder's access rules (needs `knowledge.manage`).
#[utoipa::path(put, path = "/knowledge/folders/{id}/access", tag = "Knowledge", params(("id" = Uuid, Path)), request_body = KnowledgeAccessDto, responses((status = 200, body = KnowledgeFolderDto)))]
async fn set_knowledge_folder_access(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<KnowledgeAccessDto>,
) -> ApiResult<Json<KnowledgeFolderDto>> {
    Ok(Json(
        api.service
            .set_folder_access(&p, id, b.access()?)
            .await?
            .into(),
    ))
}

// ----- Articles -----------------------------------------------------------------------------

/// A Knowledge Article.
#[derive(Debug, Serialize, ToSchema)]
pub struct KnowledgeArticleDto {
    /// ID.
    pub id: Uuid,
    /// Company, or null when shared with every Company.
    pub company_id: Option<Uuid>,
    /// Folder, or null at the root.
    pub folder_id: Option<Uuid>,
    /// Title.
    pub title: String,
    /// Sanitised HTML.
    pub body: String,
    /// Plain text, for inserting into a reply.
    pub body_text: String,
    /// Tags.
    pub tags: Vec<String>,
    /// `internal`, `customer` or `public`.
    pub audience: String,
    /// Author.
    pub author_id: Option<Uuid>,
    /// Owner.
    pub owner_id: Option<Uuid>,
    /// Published.
    pub is_published: bool,
    /// Archived.
    pub is_archived: bool,
    /// Review Date.
    pub next_review_date: Option<NaiveDate>,
    /// Current version.
    pub version: i32,
    /// Views.
    pub view_count: i64,
    /// The Problem whose Known Error it came from.
    pub source_problem_id: Option<Uuid>,
    /// Access rules.
    pub access: KnowledgeAccessDto,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<KnowledgeArticle> for KnowledgeArticleDto {
    fn from(a: KnowledgeArticle) -> Self {
        Self {
            id: a.id(),
            company_id: a.tenant_id().map(|t| t.as_uuid()),
            folder_id: a.folder_id(),
            title: a.title().to_owned(),
            body: a.body().to_owned(),
            body_text: a.body_text().to_owned(),
            tags: a.tags().to_vec(),
            audience: a.audience().as_str().to_owned(),
            author_id: a.author_id().map(|x| x.as_uuid()),
            owner_id: a.owner_id().map(|x| x.as_uuid()),
            is_published: a.is_published(),
            is_archived: a.is_archived(),
            next_review_date: a.next_review_date(),
            version: a.version(),
            view_count: a.view_count(),
            source_problem_id: a.source_problem_id(),
            access: a.access().into(),
            created_at: a.created_at(),
            updated_at: a.updated_at(),
        }
    }
}

/// Creates or edits an Article.
#[derive(Debug, Deserialize, ToSchema)]
pub struct KnowledgeArticleRequest {
    /// Company (create only); null shares it with every Company.
    pub company_id: Option<Uuid>,
    /// Title.
    pub title: String,
    /// HTML body (sanitised on save).
    #[serde(default)]
    pub body: String,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Folder, or null for the root.
    pub folder_id: Option<Uuid>,
    /// Owner (default: the author).
    pub owner_id: Option<Uuid>,
    /// `internal` (default), `customer` or `public`.
    pub audience: Option<String>,
    /// Review Date.
    pub next_review_date: Option<NaiveDate>,
}

impl KnowledgeArticleRequest {
    fn draft(self) -> Result<(Option<TenantId>, ArticleDraft), ApiError> {
        let audience = self
            .audience
            .as_deref()
            .map(str::parse::<Audience>)
            .transpose()?;
        Ok((
            self.company_id.map(TenantId::from_uuid),
            ArticleDraft {
                title: self.title,
                body: self.body,
                tags: self.tags,
                folder_id: self.folder_id,
                owner_id: self.owner_id.map(AnalystId::from_uuid),
                audience,
                next_review_date: self.next_review_date,
            },
        ))
    }
}

/// A page of Articles.
#[derive(Debug, Serialize, ToSchema)]
pub struct KnowledgeArticlePage {
    /// Articles: newest activity first, or most relevant first for a search.
    pub items: Vec<KnowledgeArticleDto>,
    /// Cursor for the next page (lists without `q` only).
    pub next_cursor: Option<String>,
}

/// Filters.
#[derive(Debug, Deserialize, IntoParams)]
pub struct KnowledgeListParams {
    /// Full-text search (web search syntax: words, "phrases", -exclusions).
    pub q: Option<String>,
    /// `live` (default), `current` (with unpublished) or `archived`; the last two need
    /// `knowledge.write`.
    pub state: Option<String>,
    /// Audience.
    pub audience: Option<String>,
    /// Folder.
    pub folder_id: Option<Uuid>,
    /// Tag.
    pub tag: Option<String>,
    /// Owner (`me` is accepted).
    pub owner: Option<String>,
    /// Only Articles whose Review Date has come.
    pub review_due: Option<bool>,
    /// Linked to this ticket (the ticket's side of FR-KNW-008).
    pub ticket_id: Option<Uuid>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

fn page(
    rows: Vec<KnowledgeArticle>,
    limit: Limit,
    searching: bool,
) -> Result<KnowledgeArticlePage, ApiError> {
    if searching {
        let mut rows = rows;
        rows.truncate(usize::try_from(limit.get()).unwrap_or(usize::MAX));
        return Ok(KnowledgeArticlePage {
            items: rows.into_iter().map(Into::into).collect(),
            next_cursor: None,
        });
    }
    let page =
        cooper_kernel::Page::from_rows(rows, limit, |a| Cursor::encode(&(a.updated_at(), a.id())))
            .map_err(|_| AppError::Internal)?;
    Ok(KnowledgeArticlePage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    })
}

/// Articles you may read (needs `knowledge.read`): access lists, Audience and Company
/// apply; a search ranks by relevance (FR-KNW-006).
#[utoipa::path(get, path = "/knowledge/articles", tag = "Knowledge", params(KnowledgeListParams), responses((status = 200, body = KnowledgeArticlePage)))]
async fn list_knowledge_articles(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<KnowledgeListParams>,
) -> ApiResult<Json<KnowledgeArticlePage>> {
    let limit = limit(q.limit)?;
    let lifecycle = match q.state.as_deref() {
        None | Some("live") => Lifecycle::Live,
        Some("current") => Lifecycle::Current,
        Some("archived") => Lifecycle::Archived,
        Some(_) => {
            return Err(
                KnowledgeError::invalid("state", "must be live, current or archived").into(),
            );
        }
    };
    let owner_id = match q.owner.as_deref() {
        None => None,
        Some("me") => match p.actor() {
            cooper_kernel::Actor::Analyst(a) => Some(a),
            _ => return Err(KnowledgeError::invalid("owner", "`me` needs an Analyst").into()),
        },
        Some(id) => Some(AnalystId::from_uuid(id.parse().map_err(|_| {
            KnowledgeError::invalid("owner", "must be an ID or `me`")
        })?)),
    };
    let after = q
        .cursor
        .map(|c| Cursor::from_client(c).decode::<(DateTime<Utc>, Uuid)>())
        .transpose()
        .map_err(|_| KnowledgeError::invalid("cursor", "is not valid"))?;
    let searching = q.q.as_deref().is_some_and(|t| !t.trim().is_empty());
    let input = ArticleListInput {
        text: q.q,
        lifecycle,
        audience: q
            .audience
            .as_deref()
            .map(str::parse::<Audience>)
            .transpose()?,
        folder_id: q.folder_id,
        tag: q.tag,
        owner_id,
        review_due: q.review_due.unwrap_or(false),
        ticket_id: q.ticket_id,
        limit,
        after,
    };
    Ok(Json(page(
        api.service.list(&p, input).await?,
        limit,
        searching,
    )?))
}

/// Creates an unpublished Article (needs `knowledge.write`).
#[utoipa::path(post, path = "/knowledge/articles", tag = "Knowledge", request_body = KnowledgeArticleRequest, responses((status = 201, body = KnowledgeArticleDto)))]
async fn create_knowledge_article(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<KnowledgeArticleRequest>,
) -> ApiResult<Response> {
    let (tenant, draft) = b.draft()?;
    Ok(created(KnowledgeArticleDto::from(
        api.service.create(&p, tenant, draft).await?,
    )))
}

/// Opens an Article and counts the view (needs `knowledge.read`; unpublished and
/// archived ones need `knowledge.write`).
#[utoipa::path(get, path = "/knowledge/articles/{id}", tag = "Knowledge", params(("id" = Uuid, Path)), responses((status = 200, body = KnowledgeArticleDto)))]
async fn get_knowledge_article(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<KnowledgeArticleDto>> {
    Ok(Json(api.service.open(&p, id).await?.into()))
}

/// Edits an Article; a new title or body saves a new version (needs `knowledge.write`).
#[utoipa::path(put, path = "/knowledge/articles/{id}", tag = "Knowledge", params(("id" = Uuid, Path)), request_body = KnowledgeArticleRequest, responses((status = 200, body = KnowledgeArticleDto)))]
async fn edit_knowledge_article(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<KnowledgeArticleRequest>,
) -> ApiResult<Json<KnowledgeArticleDto>> {
    let (_, draft) = b.draft()?;
    Ok(Json(api.service.edit(&p, id, draft).await?.into()))
}

/// Publishes or unpublishes.
#[derive(Debug, Deserialize, ToSchema)]
pub struct KnowledgePublishedRequest {
    /// Published.
    pub is_published: bool,
}

/// Publishes or unpublishes an Article (needs `knowledge.write`).
#[utoipa::path(put, path = "/knowledge/articles/{id}/published", tag = "Knowledge", params(("id" = Uuid, Path)), request_body = KnowledgePublishedRequest, responses((status = 200, body = KnowledgeArticleDto)))]
async fn set_knowledge_article_published(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<KnowledgePublishedRequest>,
) -> ApiResult<Json<KnowledgeArticleDto>> {
    Ok(Json(
        api.service
            .set_published(&p, id, b.is_published)
            .await?
            .into(),
    ))
}

/// Archives an Article; it leaves every list but keeps its history (needs
/// `knowledge.write`).
#[utoipa::path(post, path = "/knowledge/articles/{id}/archive", tag = "Knowledge", params(("id" = Uuid, Path)), responses((status = 200, body = KnowledgeArticleDto)))]
async fn archive_knowledge_article(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<KnowledgeArticleDto>> {
    Ok(Json(api.service.archive(&p, id).await?.into()))
}

/// Restores an archived Article, unpublished (needs `knowledge.write`).
#[utoipa::path(post, path = "/knowledge/articles/{id}/restore", tag = "Knowledge", params(("id" = Uuid, Path)), responses((status = 200, body = KnowledgeArticleDto)))]
async fn unarchive_knowledge_article(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<KnowledgeArticleDto>> {
    Ok(Json(api.service.unarchive(&p, id).await?.into()))
}

/// A saved version.
#[derive(Debug, Serialize, ToSchema)]
pub struct KnowledgeArticleVersionDto {
    /// Version number.
    pub version_no: i32,
    /// Title then.
    pub title: String,
    /// Body then.
    pub body: String,
    /// Who saved it (`{kind, id}`).
    #[schema(value_type = Object)]
    pub saved_by: serde_json::Value,
    /// When.
    pub saved_at: DateTime<Utc>,
}

impl From<KnowledgeArticleVersion> for KnowledgeArticleVersionDto {
    fn from(v: KnowledgeArticleVersion) -> Self {
        Self {
            version_no: v.version_no,
            title: v.title,
            body: v.body,
            saved_by: serde_json::json!({ "kind": v.saved_by.kind().as_str(), "id": v.saved_by.id() }),
            saved_at: v.saved_at,
        }
    }
}

/// Versions of an Article, newest first (FR-KNW-002).
#[utoipa::path(get, path = "/knowledge/articles/{id}/versions", tag = "Knowledge", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<KnowledgeArticleVersionDto>)))]
async fn list_knowledge_article_versions(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<KnowledgeArticleVersionDto>>> {
    Ok(Json(
        api.service
            .versions(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Restores a version as a new version (needs `knowledge.write`).
#[utoipa::path(post, path = "/knowledge/articles/{id}/versions/{version_no}/restore", tag = "Knowledge", params(("id" = Uuid, Path), ("version_no" = i32, Path)), responses((status = 200, body = KnowledgeArticleDto)))]
async fn revert_knowledge_article(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path((id, version)): Path<(Uuid, i32)>,
) -> ApiResult<Json<KnowledgeArticleDto>> {
    Ok(Json(api.service.revert(&p, id, version).await?.into()))
}

/// Replaces an Article's access rules (needs `knowledge.manage`).
#[utoipa::path(put, path = "/knowledge/articles/{id}/access", tag = "Knowledge", params(("id" = Uuid, Path)), request_body = KnowledgeAccessDto, responses((status = 200, body = KnowledgeArticleDto)))]
async fn set_knowledge_article_access(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<KnowledgeAccessDto>,
) -> ApiResult<Json<KnowledgeArticleDto>> {
    Ok(Json(
        api.service
            .set_article_access(&p, id, b.access()?)
            .await?
            .into(),
    ))
}

/// One entry of an Article's history.
#[derive(Debug, Serialize, ToSchema)]
pub struct KnowledgeHistoryDto {
    /// When.
    pub at: DateTime<Utc>,
    /// Who (`{kind, id}`).
    #[schema(value_type = Object)]
    pub actor: serde_json::Value,
    /// `created`, `updated`, `deleted` (archived), `restored`, `viewed` or
    /// `admin_override`.
    pub action: String,
    /// Field.
    pub field: Option<String>,
    /// Before.
    #[schema(value_type = Object)]
    pub old_value: Option<serde_json::Value>,
    /// After.
    #[schema(value_type = Object)]
    pub new_value: Option<serde_json::Value>,
}

impl From<HistoryEntry> for KnowledgeHistoryDto {
    fn from(e: HistoryEntry) -> Self {
        Self {
            at: e.at,
            actor: serde_json::json!({ "kind": e.actor.kind().as_str(), "id": e.actor.id() }),
            action: e.action,
            field: e.field,
            old_value: e.old_value,
            new_value: e.new_value,
        }
    }
}

/// The knowledge audit of an Article: changes, views and administrator overrides
/// (needs `knowledge.manage`; FR-KNW-009).
#[utoipa::path(get, path = "/knowledge/articles/{id}/history", tag = "Knowledge", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<KnowledgeHistoryDto>)))]
async fn knowledge_article_history(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<KnowledgeHistoryDto>>> {
    Ok(Json(
        api.service
            .history(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// A ticket linked to an Article.
#[derive(Debug, Serialize, ToSchema)]
pub struct KnowledgeTicketLinkDto {
    /// The ticket.
    pub ticket_id: Uuid,
    /// Ticket Number.
    pub number: String,
    /// When it was linked.
    pub linked_at: DateTime<Utc>,
}

impl From<(TicketLink, String)> for KnowledgeTicketLinkDto {
    fn from((l, number): (TicketLink, String)) -> Self {
        Self {
            ticket_id: l.ticket_id,
            number,
            linked_at: l.linked_at,
        }
    }
}

/// Links a ticket.
#[derive(Debug, Deserialize, ToSchema)]
pub struct KnowledgeLinkTicketRequest {
    /// The ticket.
    pub ticket_id: Uuid,
}

/// Tickets linked to an Article that you may see.
#[utoipa::path(get, path = "/knowledge/articles/{id}/tickets", tag = "Knowledge", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<KnowledgeTicketLinkDto>)))]
async fn list_knowledge_article_tickets(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<KnowledgeTicketLinkDto>>> {
    Ok(Json(
        api.service
            .tickets(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Links an Article to a ticket (needs `knowledge.write`; FR-KNW-008).
#[utoipa::path(post, path = "/knowledge/articles/{id}/tickets", tag = "Knowledge", params(("id" = Uuid, Path)), request_body = KnowledgeLinkTicketRequest, responses((status = 201, body = KnowledgeTicketLinkDto)))]
async fn link_knowledge_article_ticket(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<KnowledgeLinkTicketRequest>,
) -> ApiResult<Response> {
    Ok(created(KnowledgeTicketLinkDto::from(
        api.service.link_ticket(&p, id, b.ticket_id).await?,
    )))
}

/// Removes a ticket link (needs `knowledge.write`).
#[utoipa::path(delete, path = "/knowledge/articles/{id}/tickets/{ticket_id}", tag = "Knowledge", params(("id" = Uuid, Path), ("ticket_id" = Uuid, Path)), responses((status = 204)))]
async fn unlink_knowledge_article_ticket(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path((id, ticket)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service.unlink_ticket(&p, id, ticket).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- Portal (FR-PRT-005) ----------------------------------------------------------------------

/// An Article as the Portal shows it.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalArticleDto {
    /// ID.
    pub id: Uuid,
    /// Title.
    pub title: String,
    /// Sanitised HTML.
    pub body: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<KnowledgeArticle> for PortalArticleDto {
    fn from(a: KnowledgeArticle) -> Self {
        Self {
            id: a.id(),
            title: a.title().to_owned(),
            body: a.body().to_owned(),
            tags: a.tags().to_vec(),
            updated_at: a.updated_at(),
        }
    }
}

/// A Portal search.
#[derive(Debug, Deserialize, IntoParams)]
pub struct PortalKnowledgeParams {
    /// Full-text search; without it, the newest Articles.
    pub q: Option<String>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
}

/// Knowledge Articles for the signed-in Requester: live, `customer` or `public`, of
/// their Company or shared, and allowed by the access lists.
#[utoipa::path(get, path = "/portal/knowledge", tag = "Portal", params(PortalKnowledgeParams),
    responses((status = 200, body = [PortalArticleDto]), (status = 401, description = "Not signed in to the Portal")))]
async fn search_portal_knowledge(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<PortalKnowledgeParams>,
) -> ApiResult<Json<Vec<PortalArticleDto>>> {
    let limit = limit(q.limit)?;
    let mut rows = api.service.portal_search(&p, q.q, limit).await?;
    rows.truncate(usize::try_from(limit.get()).unwrap_or(usize::MAX));
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

/// Reads one Article on the Portal and counts the view.
#[utoipa::path(get, path = "/portal/knowledge/{id}", tag = "Portal", params(("id" = Uuid, Path)),
    responses((status = 200, body = PortalArticleDto), (status = 404, description = "Not found or not readable")))]
async fn read_portal_knowledge_article(
    State(api): State<KnowledgeApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PortalArticleDto>> {
    Ok(Json(api.service.portal_open(&p, id).await?.into()))
}
