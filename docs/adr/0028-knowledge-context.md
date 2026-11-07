# 0028. Knowledge: one read decision, access lists with object polarity, shared Articles

- **Status:** Accepted
- **Date:** 2026-11-07
- **Issue:** #131, #132, #133, #134, #135, #136, #137, #138, #108, #114
- **Requirements:** FR-KNW-001 to FR-KNW-009, FR-PRT-005, FR-PRB-005

## Context

FreeITSM's knowledge base grew several readers (the article list, the article page, the
Portal Help Centre, web chat, the AI assistant). Each one put together its own
visibility filter from three parts: the lifecycle (`is_published`, `is_archived`), the
Company and the Audience. FreeITSM's own notes say this is how archived Articles once
reached anonymous readers. Folders and access lists came later. FreeITSM then made one
resolver the only place that decides who reads what, with these rules:

- **Every axis narrows.** No axis widens what another one allows.
- **The node holds the polarity.** An *Open* node admits everyone except the principals
  it names. A *Restricted* node admits only the principals it names. A grant and a
  denial can therefore never contradict each other.
- **Inheritance.** A node that inherits ignores its own rows.
- **Containers.** Every Folder above an Article must admit the reader.
- **Administrator floor.** People who manage Knowledge always get in, and that is
  audited.

For Knowledge, a NULL Company means *shared with every Company*. For tickets, a NULL
Company means something else.

## Decision

- A new crate `cooper-knowledge` (bounded context, AGENTS.md §3) behind the
  **Knowledge** Module: `/knowledge/…` and the Portal's `/portal/knowledge…`. For a given
  path, the longest matching prefix decides which Module owns it. Switching Knowledge
  off therefore also hides the Portal pages.
- **One read decision.** Every reader goes through `KnowledgeService::readable` (one
  Article) or `search` (lists). Both use the same domain functions:
  - **Lifecycle:** readers see live Articles (published and not archived). Unpublished
    and archived Articles need `knowledge.write`.
  - **Audience ladder:** Analysts read every Audience. Requesters read `customer` and
    `public`. `public` exists for future anonymous readers.
  - **Company:** the Article's Company, or shared.
  - **Access lists:** `acl_decision` and `FolderTree::readable`. Every axis is ANDed with
    the others. An Article that is not readable answers 404, whatever the reason.
- **Access lists** follow FreeITSM's model. `is_restricted` and `inherit_permissions`
  are stored on Folders and Articles. `knowledge_acl` holds rows naming analysts,
  teams, requesters or User Groups. Expired User Group memberships do not count; the
  identity context gives this through `groups_of`. The walk climbs Folders to the root,
  and a cycle fails closed. For lists, the reader's blocked Folders and Articles are
  worked out once in Rust from the small Folder tree, then passed to SQL as ID arrays.
  This is not a recursive query per row, and the result is not cached in a table.
- **The administrator floor** is the `knowledge.manage` Capability. Opening an Article
  that only the floor let you read writes an `admin_override` row. Lists do not write
  one: browsing is not an override.
- **Shared rows.** Folders, Articles, versions, access lists and the access log have a
  nullable `tenant_id`. Their RLS policy is `tenant_id IS NULL OR
  cooper_tenant_visible(tenant_id)`. Only callers who see every Company may create shared
  content. A Company Folder holds only that Company's content. A shared Folder holds
  anything. Ticket links always carry the ticket's Company.
- **Versions.** Every save that changes the title or the body adds an immutable
  `knowledge_article_versions` row (a trigger rejects `UPDATE` and `DELETE`). Restoring
  an old version saves it as a new version.
- **Sanitising.** `ammonia` cleans the HTML when it is saved: no scripts, styles, event
  handlers or unsafe URL schemes, and links get `rel="noopener noreferrer"`. The plain
  text is kept next to the HTML. It feeds the search vector and is what an Analyst
  inserts into a reply.
- **Search.** A stored `tsvector` built from the title (weight A) and the body text
  (weight B), with a GIN index, `websearch_to_tsquery('english', …)` and `ts_rank`
  ordering. Ranked searches return one page without a cursor. Plain lists page by
  `(updated_at, id)`.
- **Lifecycle.** Publish and unpublish. Archiving is the recycle bin: it unpublishes,
  blocks edits and keeps the history. Restoring brings the Article back unpublished.
  Nothing is hard-deleted, so the audit stays complete.
- **The knowledge audit.** Changes go to the platform audit trail
  (`knowledge_article`, `knowledge_folder`): create, edit, delete (archive), restore,
  move, `access`, ticket links. Views and administrator overrides are high-volume, so
  they go to an append-only `knowledge_access_log`. `view_count` counts views.
  `GET /knowledge/articles/{id}/history` merges both logs.
- **Review Dates.** The `knowledge-review` job raises `knowledge_article.review_due`
  once per date, through the outbox. A Notification of kind `article_review_due` (with
  `article_id`) goes to the Owner. `?owner=me&review_due=true` lists Articles past their
  Review Date.
- **Tickets.** `knowledge_article_tickets` links tickets the caller may see; tickets are
  reached through the service desk's application API. The ticket's side is
  `GET /knowledge/articles?ticket_id=`. To insert an Article into a reply, the client
  adds `body_text` to its reply box. The reply endpoint stays as it is.
- **Known Errors (FR-PRB-005).** `POST /problems/{id}/knowledge-article` needs
  `problems.update`, and Knowledge checks `knowledge.write`. It goes through a
  `Knowledge` port and creates an internal, unpublished Article in the Problem's Company,
  from the Root Cause and the Workaround (escaped). `source_problem_id` records the
  Problem. Publishing again saves a new version of the same Article.

## Deviations from FreeITSM

- **One permission model.** FreeITSM can be switched install-wide between `containers`
  and `filing`. Cooper has only `containers` (a locked cabinet is locked). In the
  `filing` model, an Article's own grant can reach past a restricted Folder, which is
  the widening that the "every axis narrows" rule forbids. One model also gives one
  answer to "can Bob read this?".
- **Archiving unpublishes.** In FreeITSM, archiving and publishing are independent, so
  every reader must check both flags. Cooper still checks both, but archiving clears
  `is_published` and restoring brings the Article back unpublished. A restored Article
  never goes live by surprise.
- **New Articles start unpublished.** In FreeITSM, `is_published` defaults to 1.
- **Shortcuts, the AI assistant, gap analysis and the share-by-email feature are not part
  of this decision.** Knowledge Gap is FR-KNW-010, planned for P4.

## Consequences

- New dependency `ammonia` (MIT/Apache-2.0). Its CSS parser crates `cssparser` and
  `dtoa-short` are MPL-2.0, allowed by a per-crate exception in `deny.toml`. We use them
  unmodified, and MPL is file-level copyleft. Older ammonia releases without them are
  affected by RUSTSEC-2021-0157.

- A new reader (web chat, AI answers) gets the same visibility as everyone else by
  calling `search` or `readable`. It cannot assemble its own filter.
- Folder trees are loaded per request. That is fine for the expected size (tens to
  hundreds of Folders). If installations grow far beyond that, the blocked set can be
  memoised per request without changing the model.
