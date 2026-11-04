# 0006. Use PostgreSQL full-text search, behind a per-context search port

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issue:** #6
- **Requirements:** FR-GEN-003, FR-TKT-014, FR-TKT-041, FR-KNW-006, FR-EML-007; Appendix A

## Context

Search covers:

- tickets by number (current or historical), subject, body, Requester, email address and
  extracted attachment text (FR-TKT-041);
- Knowledge Articles, respecting audience and ACLs (FR-KNW-006);
- every list (FR-GEN-003).

Results must respect tenant isolation, Confidential tickets (FR-TKT-014) and Knowledge
ACLs. The performance target is P95 < 300 ms at 1M tickets. FreeITSM uses MySQL `LIKE`
and FULLTEXT queries spread across pages.

## Options considered

1. **PostgreSQL full-text search:**
   - `tsvector` columns maintained by generated columns or triggers, with GIN indexes;
   - `websearch_to_tsquery` for user input;
   - `pg_trgm` for partial matches on numbers, email addresses and names.
   - Pros: transactional, no extra infrastructure, and permission filters (tenant, RLS,
     confidentiality, ACL joins) apply in the same query.
   - Cons: weaker relevance tuning and stemming than dedicated engines, and the search
     load lands on the primary database.
2. **External engine** (OpenSearch, Meilisearch, Tantivy):
   - Pros: better ranking, typo tolerance and facets.
   - Cons: another system to run, eventual consistency, and every permission rule has to
     be duplicated in the index. That is the riskiest part for Confidential tickets and
     ACLs.
3. **Hybrid:** PostgreSQL now, an external engine later behind the same interface.

## Decision

Option 3, starting with **PostgreSQL**:

- Each context that offers search defines a **search port** (a trait in its
  `application` layer, such as `TicketSearch`). Its implementation in `infrastructure`
  uses PostgreSQL FTS, so a later engine swap stays local to one module.
- Searchable text lives in a `search_vector tsvector` column per searchable table. It is
  built with `setweight` (subject or title A, body B, other C) and indexed with GIN. The
  text-search configuration is `simple` plus `unaccent` by default, because Companies
  write in many languages. A per-Company language setting can come later.
- Exact and prefix lookups (ticket numbers including Ticket Number History, email
  addresses) use B-tree or `pg_trgm` indexes, not `tsvector`.
- Extracted attachment text (FR-EML-007) is stored in its own table with its own
  `tsvector` and joined in, so large documents do not bloat ticket rows.
- Permission filters are part of the SQL query: tenant filter + RLS, the Confidential
  rule, and the Knowledge ACL join. No result is filtered after the fact in Rust, which
  would break paging and counts.

## Consequences

- Search arrives with each feature (tickets in P1, knowledge in P2) without new
  infrastructure.
- Migrations must add the `unaccent` and `pg_trgm` extensions when the first searchable
  table lands. Managed PostgreSQL offerings support both.
- The P2 performance suite measures search at 1M tickets. If ranking quality or load
  becomes a problem, a new ADR can introduce an external engine behind the existing
  ports. Its index must then carry tenant, sensitivity and ACL data.
