# 0010. Build the Analyst console and the Portal as a TypeScript React SPA served by `cooper-server`

- **Status:** Accepted
- **Date:** 2026-11-05
- **Supersedes:** [ADR-0003](0003-server-rendered-ui-with-askama-and-htmx.md)
- **Issue:** #234 (follow-up work: #232, #233, #81, #82, #83)
- **Requirements:** FR-API-001, FR-GEN-005, FR-GEN-006, FR-GEN-008, FR-GEN-009,
  FR-TKT-024, FR-TKT-025, FR-TKT-040, FR-SLA-007, FR-PRT-001, FR-SST-009;
  Appendix A (WCAG 2.1 AA)

## Context

[ADR-0003](0003-server-rendered-ui-with-askama-and-htmx.md) chose askama + htmx. It
optimised for one toolchain and simple operations. No UI code has been written under it
yet, so changing course now is cheap.

The project goal is a best-in-class Analyst experience. An ITSM console is used all day by
the same people, and what makes it good is mostly client-side interaction:

- dense, sortable, filterable lists with thousands of rows and smooth scrolling
  (FR-TKT-040, FR-SLA-007), and saved Table Views (FR-GEN-008);
- keyboard-first triage, a command palette (⌘K / Ctrl+K) and a split-pane ticket view;
- instant navigation, cached data and optimistic updates;
- live Ticket Presence (FR-TKT-025) and Notifications (FR-GEN-006);
- rich screens that P2–P4 need: the Kanban board for Tasks, the CMDB graph, dashboards and
  the Form builder.

With htmx, each of these becomes a hand-built JavaScript island, with no shared state,
component model or test tooling. The React ecosystem has accessible, maintained parts for
all of them.

The concerns that drove ADR-0003 are still valid and must stay solved:

- no business rules in the UI (FreeITSM duplicated change risk banding in PHP and JS);
- every UI capability available in the REST API (FR-API-001);
- WCAG 2.1 AA (Appendix A);
- one deployable and a small attack surface.

## Options considered

1. **Keep askama + htmx** (optionally with Alpine.js or Datastar for client state).
   - Pros: one language and build; semantic HTML by default; fast first paint.
   - Cons: the rich screens above need bespoke JavaScript with no component model; few
     ready-made accessible data grids, palettes or graph editors; harder to hire for.
2. **Rust to WebAssembly (Leptos or Dioxus).**
   - Pros: one language; types could be shared with the backend.
   - Cons: component libraries, accessibility primitives and data grids lag far behind
     React's; larger bundles; a small hiring pool. Sharing domain types with the UI would
     also tempt us to put business rules in the client.
3. **SvelteKit or Vue/Nuxt.**
   - Pros: technically strong, smaller bundles than React.
   - Cons: smaller ecosystem for the specific parts we need (shadcn/ui, TanStack,
     React Flow); SvelteKit and Nuxt default to their own server, which we do not want.
4. **TypeScript + React single-page app (SPA), built to static files and served by
   `cooper-server`.**
   - Pros: the widest ecosystem of accessible components; type-safe routing and data
     fetching; mature testing tools; the UI exercises the same API integrators use.
   - Cons: a second toolchain (Node + pnpm); client-side state needs discipline; first
     paint depends on JavaScript (acceptable for a signed-in console).
5. **React meta-framework with its own server (Next.js, Remix/React Router framework
   mode).** Rejected: it adds a second backend server and runtime next to axum, splits
   authentication and authorization, and complicates deployment.

## Decision

Option 4. Both the Analyst console and the Portal are one TypeScript React SPA with two
route trees (`/` for Analysts, `/portal` for Requesters), sharing one design system.

### Stack

| Concern | Choice |
|---|---|
| Language | TypeScript, `strict` |
| UI library | React 19 |
| Build and dev server | Vite (static output, dev proxy to `cooper-server`) |
| Package manager | pnpm, with the lockfile committed; Node version pinned in `web/.node-version` and `package.json` `engines` |
| Routing | TanStack Router (type-safe routes; list filters, sort and selection live in the URL) |
| Server state | TanStack Query (caching, background refresh, optimistic updates). No global client store unless a later ADR justifies one |
| Tables | TanStack Table + TanStack Virtual |
| Components | shadcn/ui (copied into the repo) on Radix primitives, Tailwind CSS v4 with design tokens; light and dark themes |
| Forms | React Hook Form with schemas generated from the OpenAPI description |
| API client | Types and client generated from the `utoipa` OpenAPI document (#80). A changed API breaks the UI build, not the running app |
| Live updates | Server-Sent Events from axum. Events invalidate TanStack Query caches; they do not carry state for the client to merge |
| Rich text | Tiptap. HTML is sanitised on the server with `ammonia` (AGENTS.md §10) |
| Specialised screens | dnd-kit (Kanban), React Flow (CMDB graph), Apache ECharts (dashboards) |
| Localisation | FormatJS (ICU messages); all text goes through the catalogue from the first screen (FR-GEN-009) |
| Lint and format | Biome |
| Tests | Vitest + Testing Library (components), Playwright (end-to-end), axe-core in both (WCAG 2.1 AA), Storybook for the design system with screenshot tests |

### Rules

- **No business rules in the UI.** SLA times, risk levels, permissions, validation and
  state transitions are computed in Rust and returned by the API. Client-side checks only
  make forms friendlier; the server is always authoritative.
- **API only.** The UI talks to `/api/v1` and the SSE endpoint, nothing else. If the UI
  needs something, it is added to the API first. This enforces FR-API-001.
- **Capabilities drive the UI.** The API returns the signed-in user's Capabilities and
  enabled Modules (FR-GEN-005). The UI hides what the user cannot do; the server still
  checks every request.
- **One deployable.** `web/` is built to static files that are embedded into the
  `cooper-server` binary and served from the same origin, with an SPA fallback to
  `index.html` for non-API paths. No separate web server and no CORS for the UI.
- **Sessions, not tokens, in the browser.** Sign-in uses an `HttpOnly`, `Secure`,
  `SameSite=Lax` session cookie plus a CSRF token on unsafe methods. No tokens in
  `localStorage`. API keys remain for integrations only (AGENTS.md §6).
- **Strict Content-Security-Policy.** Hashed, same-origin scripts only; no inline scripts
  or `eval`. Bundles are content-hashed and served with long-lived cache headers;
  `index.html` is never cached.
- **Vocabulary applies.** Route paths, component names and UI copy use the terms in
  `docs/VOCABULARY.md` (Analyst, Requester, Company, and so on).
- **Exception: the public status page** (FR-SST-009) is server-rendered with askama. It is
  small, unauthenticated, must work without JavaScript, and must be cacheable.

### Layout

```
web/
├── package.json, pnpm-lock.yaml, .node-version, vite.config.ts, biome.json
├── src/
│   ├── api/          # generated client and types (not edited by hand)
│   ├── components/   # design system (shadcn/ui based)
│   ├── features/     # one folder per bounded context: tickets/, changes/, ...
│   ├── routes/       # TanStack Router route tree: analyst and portal
│   └── i18n/         # message catalogues
└── e2e/              # Playwright tests
```

## Consequences

- **New toolchain.** Contributors need Node (pinned version) and pnpm. `just` gains
  `web-dev`, `web-check` and `web-build` recipes, and the Dockerfile gains a Node build
  stage whose output is embedded into `cooper-server`.
- **CI** gains a `web` job: install with the frozen lockfile, typecheck, Biome, Vitest,
  build, and Playwright with axe-core against a running `cooper-server`. A check fails the
  build if the generated API client is out of date.
- **Staying current.** Dependabot gets an `npm` entry with grouped weekly updates;
  Storybook screenshot tests catch unintended visual changes from upgrades. Only stable
  releases are used for core screens (no canary or pre-1.0 frameworks). The stack is
  reviewed when a major version of React, TanStack or Tailwind ships, and changes are
  recorded as ADRs.
- **Supply chain.** npm dependencies are a new risk. Mitigations: committed lockfile,
  `pnpm install --frozen-lockfile`, pnpm's lifecycle-script allow-list, `pnpm audit` in
  CI, and Dependabot security updates. Adding a runtime npm dependency needs a sentence
  in the PR explaining why, as for Rust crates ([ADR-0009](0009-shared-crates-and-dependency-baseline.md)).
- **API surface first.** The OpenAPI description (#80) and the session/CSRF endpoints
  become prerequisites for every UI issue.
- **UX work is part of the UI issues.** Screens are designed from Analyst workflows
  (triage, reply, assign, close) and checked with users, not only implemented from the
  requirement text.
- **Revisit** if bundle size or first paint becomes a real problem for the Portal; a
  server-rendered Portal could then be reconsidered in a new ADR.
