# 0003. Server-rendered UI with askama and htmx

- **Status:** Superseded by [0010](0010-typescript-react-spa-for-analyst-console-and-portal.md)
- **Date:** 2026-11-04
- **Issue:** #3 (Appendix B, open question 3)
- **Requirements:** FR-GEN-005, FR-GEN-009, FR-TKT-025, FR-PRT-001; Appendix A (WCAG 2.1 AA)

## Context

Cooper needs two UIs: the Analyst console and the Self-Service Portal. FreeITSM renders
PHP pages and enhances them with jQuery-style scripts, with business rules sometimes
living only in JavaScript (for example the duplicated change risk banding). Requirements
that shape the choice:

- **Every UI capability is also in the REST API** (FR-API-001). The UI must not hold
  business rules.
- **Accessibility:** WCAG 2.1 AA (Appendix A).
- **Live features:** Ticket Presence (FR-TKT-025), notifications (FR-GEN-006) and
  dashboards.
- **Localisation** of UI and email texts (FR-GEN-009).
- **Operations:** one Rust toolchain and one deployable are strongly preferred.

## Options considered

1. **Server-rendered HTML (askama templates) + htmx**, with small islands of JavaScript
   where needed (rich-text editor, drag and drop, charts).
   - Pros:
     - One language and one build, and templates are type-checked at compile time.
     - Pages are accessible by default because they are semantic HTML.
     - Progressive enhancement.
     - Fast first paint.
     - Server-Sent Events cover presence and notifications.
     - No duplicated validation, because rules stay in Rust.
   - Cons:
     - Very interactive screens (Kanban board, CMDB graph) need targeted JavaScript.
     - Fewer front-end specialists know htmx than React.
2. **SPA (React/TypeScript or Leptos/WASM) served by axum**, talking only to `/api/v1`.
   - Pros:
     - Rich interactivity.
     - The API is exercised by our own UI.
   - Cons:
     - A second toolchain (Node or WASM), a larger attack surface, and client-side state
       bugs.
     - Accessibility needs constant discipline.
     - Two places to keep in step with every requirement.
3. **Both:** server-rendered Portal, SPA for Analysts. This doubles the cost.

## Decision

Build both UIs **server-rendered with askama templates and htmx**, served by
`cooper-server`:

- Handlers for HTML call the **same application services** as the JSON API handlers. No
  business logic lives in templates or scripts.
- Live updates use **Server-Sent Events** (presence, notifications, dashboard tiles).
- Interactive islands (rich-text editor for Knowledge, Kanban drag and drop, charts, CMDB
  graph) use small, self-contained ES modules, vendored and pinned. There is no Node
  build step unless an ADR introduces one.
- Text goes through a message catalogue from the start, so localisation (FR-GEN-009) is
  a translation task, not a refactor.
- Accessibility is checked in CI with axe-core (issue "Automated accessibility checks").

## Consequences

- The UI work begins in P1 with the Analyst shell and the Portal shell. Their issues now
  assume askama + htmx.
- The REST API stays first-class for integrations, and the OpenAPI document is generated
  from the same DTOs.
- If a future screen proves impossible to build well with htmx plus islands, a later ADR
  can introduce an SPA for that screen only. This ADR does not forbid it.
