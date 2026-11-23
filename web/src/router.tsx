import { QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  lazyRouteComponent,
  Outlet,
  type RouteComponent,
  redirect,
} from "@tanstack/react-router";
import { SignInPage } from "./features/auth/SignIn";
import { analystSessionQuery, portalSessionQuery } from "./features/session/session";
import { AnalystLayout } from "./features/shell/AnalystLayout";
import { NotFound } from "./features/shell/NotFound";
import { parseTicketSearch } from "./features/tickets/views";
import { isDayKey, parseMonth } from "./lib/dates";
import { compact, flag, oneOf, text } from "./routes/search";

/* biome-ignore-start lint/suspicious/noExplicitAny: pages take their props from the route wrappers below */
type Page = ((props: any) => React.ReactNode) & { preload?: (() => Promise<void>) | undefined };
/* biome-ignore-end lint/suspicious/noExplicitAny: end */

/** A screen loaded on demand, so the first download holds only the frame. */
const lazy = (load: () => Promise<Record<string, unknown>>, name: string): Page =>
  lazyRouteComponent(load as never, name as never) as unknown as Page;

/** A lazy page used directly as a route component (it takes no props). */
const screen = (page: Page): RouteComponent => page as unknown as RouteComponent;

/** A route component that renders a lazy page with route props; preloaded with the route. */
function withPage(page: Page, component: () => React.JSX.Element): RouteComponent {
  return page.preload ? Object.assign(component, { preload: page.preload }) : component;
}

const P = {
  ResetPassword: lazy(() => import("./features/auth/ResetPassword"), "ResetPasswordPage"),
  TwoFactor: lazy(() => import("./features/auth/TwoFactorSetup"), "TwoFactorSetupPage"),
  Setup: lazy(() => import("./features/auth/Setup"), "SetupPage"),
  Home: lazy(() => import("./features/home/HomePage"), "HomePage"),
  Notifications: lazy(
    () => import("./features/notifications/NotificationsPage"),
    "NotificationsPage",
  ),
  Account: lazy(() => import("./features/account/AccountPage"), "AccountPage"),
  Tickets: lazy(() => import("./features/tickets/TicketsPage"), "TicketsPage"),
  Ticket: lazy(() => import("./features/tickets/TicketDetail"), "TicketDetail"),
  NewTicket: lazy(() => import("./features/tickets/NewTicket"), "NewTicket"),
  Tasks: lazy(() => import("./features/tasks/TasksPage"), "TasksPage"),
  Task: lazy(() => import("./features/tasks/TaskPage"), "TaskPage"),
  Assets: lazy(() => import("./features/assets/AssetsPage"), "AssetsPage"),
  Asset: lazy(() => import("./features/assets/AssetPage"), "AssetPage"),
  Contracts: lazy(() => import("./features/contracts/ContractsPage"), "ContractsPage"),
  Contract: lazy(() => import("./features/contracts/ContractPage"), "ContractPage"),
  Suppliers: lazy(() => import("./features/contracts/SuppliersPage"), "SuppliersPage"),
  Supplier: lazy(() => import("./features/contracts/SupplierPage"), "SupplierPage"),
  Knowledge: lazy(() => import("./features/knowledge/KnowledgePage"), "KnowledgePage"),
  Article: lazy(() => import("./features/knowledge/ArticlePage"), "ArticlePage"),
  Calendar: lazy(() => import("./features/calendar/CalendarPage"), "CalendarPage"),
  StatusLayout: lazy(
    () => import("./features/service-status/ServiceStatusLayout"),
    "ServiceStatusLayout",
  ),
  StatusOverview: lazy(() => import("./features/service-status/OverviewPage"), "OverviewPage"),
  Incidents: lazy(() => import("./features/service-status/IncidentsPage"), "IncidentsPage"),
  Incident: lazy(() => import("./features/service-status/IncidentPage"), "IncidentPage"),
  Services: lazy(() => import("./features/service-status/ServicesPage"), "ServicesPage"),
  ChecksLayout: lazy(
    () => import("./features/morning-checks/MorningChecksLayout"),
    "MorningChecksLayout",
  ),
  Round: lazy(() => import("./features/morning-checks/RoundPage"), "RoundPage"),
  Checks: lazy(() => import("./features/morning-checks/ChecksPage"), "ChecksPage"),
  UsersLayout: lazy(() => import("./features/users/UsersLayout"), "UsersLayout"),
  Analysts: lazy(() => import("./features/users/AnalystsPage"), "AnalystsPage"),
  Analyst: lazy(() => import("./features/users/AnalystPage"), "AnalystPage"),
  Requesters: lazy(() => import("./features/users/RequestersPage"), "RequestersPage"),
  Requester: lazy(() => import("./features/users/RequesterPage"), "RequesterPage"),
  AdminLayout: lazy(
    () => import("./features/administration/AdministrationLayout"),
    "AdministrationLayout",
  ),
  AdminIndex: lazy(
    () => import("./features/administration/AdministrationLayout"),
    "AdministrationIndex",
  ),
  Settings: lazy(() => import("./features/administration/SettingsPage"), "SettingsPage"),
  Modules: lazy(() => import("./features/administration/ModulesPage"), "ModulesPage"),
  Jobs: lazy(() => import("./features/administration/JobsPage"), "JobsPage"),
  SystemLog: lazy(() => import("./features/administration/SystemLogPage"), "SystemLogPage"),
  Reference: lazy(() => import("./features/administration/ReferencePage"), "ReferencePage"),
  Categories: lazy(() => import("./features/administration/CategoriesPage"), "CategoriesPage"),
  Mailboxes: lazy(() => import("./features/administration/MailboxesPage"), "MailboxesPage"),
  SuggestedReplyInstructions: lazy(
    () => import("./features/administration/SuggestedReplyInstructionsPage"),
    "SuggestedReplyInstructionsPage",
  ),
  Companies: lazy(() => import("./features/administration/CompaniesPage"), "CompaniesPage"),
  Teams: lazy(() => import("./features/administration/TeamsPage"), "TeamsPage"),
  Departments: lazy(() => import("./features/administration/DepartmentsPage"), "DepartmentsPage"),
  CostCentres: lazy(() => import("./features/administration/CostCentresPage"), "CostCentresPage"),
  Roles: lazy(() => import("./features/administration/RolesPage"), "RolesPage"),
  PortalLayout: lazy(() => import("./features/portal/PortalLayout"), "PortalLayout"),
  PortalSignIn: lazy(() => import("./features/portal/PortalAuth"), "PortalSignInPage"),
  PortalReset: lazy(() => import("./features/portal/PortalAuth"), "PortalResetPage"),
  PortalSetPassword: lazy(() => import("./features/portal/PortalAuth"), "PortalSetPasswordPage"),
  PortalRegister: lazy(() => import("./features/portal/PortalAuth"), "PortalRegisterPage"),
  PortalHome: lazy(() => import("./features/portal/PortalHome"), "PortalHome"),
  PortalTickets: lazy(() => import("./features/portal/PortalTickets"), "PortalTicketsPage"),
  PortalNewTicket: lazy(() => import("./features/portal/PortalNewTicket"), "PortalNewTicketPage"),
  PortalTicket: lazy(() => import("./features/portal/PortalTicket"), "PortalTicketPage"),
  PortalKnowledge: lazy(() => import("./features/portal/PortalKnowledge"), "PortalKnowledgePage"),
  PortalArticle: lazy(() => import("./features/portal/PortalKnowledge"), "PortalArticlePage"),
  PortalStatus: lazy(() => import("./features/portal/PortalStatus"), "PortalStatusPage"),
  PortalNotifications: lazy(
    () => import("./features/portal/PortalNotifications"),
    "PortalNotificationsPage",
  ),
};

/** What every route can use: the query cache, for loading data before rendering. */
export interface RouterContext {
  queryClient: QueryClient;
}

/**
 * Two route trees in one app (ADR-0010): the Analyst console at `/` and the Self-Service
 * Portal at `/portal`. Paths use Vocabulary terms. List screens are layouts whose child
 * routes (`$id`, `new`) open in the detail pane (master-detail), so filters in the URL
 * survive opening and closing records.
 */
const rootRoute = createRootRouteWithContext<RouterContext>()({ component: Outlet });

const redirectSearch = (s: Record<string, unknown>): { redirect?: string } =>
  compact({ redirect: text(s, "redirect") });
const tokenSearch = (s: Record<string, unknown>): { token?: string } =>
  compact({ token: text(s, "token") });

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  validateSearch: redirectSearch,
  component: function SignInRoute() {
    return <SignInPage redirect={signInRoute.useSearch().redirect} />;
  },
});
const resetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/reset-password",
  validateSearch: tokenSearch,
  component: withPage(P.ResetPassword, function ResetRoute() {
    return <P.ResetPassword token={resetRoute.useSearch().token} />;
  }),
});
const setupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/setup",
  component: screen(P.Setup),
});
const twoFactorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/two-factor/setup",
  beforeLoad: async ({ context }) => {
    if (!(await context.queryClient.ensureQueryData(analystSessionQuery)))
      throw redirect({ to: "/sign-in" });
  },
  component: screen(P.TwoFactor),
});

/** The signed-in Analyst console: sign-in and required two-factor setup come first. */
const analystRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "analyst",
  beforeLoad: async ({ context, location }) => {
    const session = await context.queryClient.ensureQueryData(analystSessionQuery);
    if (!session) throw redirect({ to: "/sign-in", search: { redirect: location.href } });
    if (session.totp_enrollment_required) throw redirect({ to: "/two-factor/setup" });
  },
  component: AnalystLayout,
});

const homeRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/",
  component: screen(P.Home),
});
const notificationsRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/notifications",
  validateSearch: (s: Record<string, unknown>) => compact({ all: flag(s, "all") }),
  component: withPage(P.Notifications, function NotificationsRoute() {
    const navigate = notificationsRoute.useNavigate();
    return (
      <P.Notifications
        all={Boolean(notificationsRoute.useSearch().all)}
        onShowChange={(all: boolean) => void navigate({ search: all ? { all: true } : {} })}
      />
    );
  }),
});
const accountRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/account",
  component: screen(P.Account),
});

/** Tickets: the inbox with the open ticket beside it (FR-TKT-040). */
const ticketsRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/tickets",
  validateSearch: parseTicketSearch,
  component: withPage(P.Tickets, function TicketsRoute() {
    return <P.Tickets search={ticketsRoute.useSearch()} />;
  }),
});
const newTicketRoute = createRoute({
  getParentRoute: () => ticketsRoute,
  path: "new",
  /** `task`: the Task the new ticket is linked to once created (FR-TSK-004). */
  validateSearch: (s: Record<string, unknown>) => compact({ task: text(s, "task") }),
  component: withPage(P.NewTicket, function NewTicketRoute() {
    return <P.NewTicket taskId={newTicketRoute.useSearch().task} />;
  }),
});
const ticketRoute = createRoute({
  getParentRoute: () => ticketsRoute,
  path: "$ticketId",
  component: withPage(P.Ticket, function TicketRoute() {
    const { ticketId } = ticketRoute.useParams();
    return <P.Ticket key={ticketId} ticketId={ticketId} />;
  }),
});

/**
 * A list screen with URL search state whose New form and records open beside it:
 * `/base`, `/base/new` and `/base/$id`.
 */
function recordRoutes<
  B extends "/tasks" | "/assets" | "/contracts" | "/suppliers" | "/knowledge",
  S,
>(base: B, List: Page, Detail: Page, validateSearch: (s: Record<string, unknown>) => S) {
  const listRoute = createRoute({
    getParentRoute: () => analystRoute,
    path: base,
    validateSearch,
    component: withPage(List, function ListRoute() {
      return <List search={listRoute.useSearch()} />;
    }),
  });
  const newRoute = createRoute({
    getParentRoute: () => listRoute,
    path: "new",
    component: withPage(Detail, function NewRoute() {
      return <Detail />;
    }),
  });
  const byIdRoute = createRoute({
    getParentRoute: () => listRoute,
    path: "$id",
    component: withPage(Detail, function DetailRoute() {
      const { id } = byIdRoute.useParams() as { id: string };
      return <Detail key={id} id={id} />;
    }),
  });
  return listRoute.addChildren([newRoute, byIdRoute]);
}

const tasksRoutes = recordRoutes("/tasks", P.Tasks, P.Task, (s) =>
  compact({ q: text(s, "q"), mine: flag(s, "mine"), closed: flag(s, "closed") }),
);
const assetsRoutes = recordRoutes("/assets", P.Assets, P.Asset, (s) =>
  compact({
    q: text(s, "q"),
    type: text(s, "type"),
    status: text(s, "status"),
    location: text(s, "location"),
    retired: flag(s, "retired"),
  }),
);
const contractsRoutes = recordRoutes("/contracts", P.Contracts, P.Contract, (s) =>
  compact({
    q: text(s, "q"),
    status: text(s, "status"),
    supplier: text(s, "supplier"),
    notice: flag(s, "notice"),
  }),
);
const suppliersRoutes = recordRoutes("/suppliers", P.Suppliers, P.Supplier, (s) =>
  compact({ q: text(s, "q") }),
);
const knowledgeRoutes = recordRoutes("/knowledge", P.Knowledge, P.Article, (s) =>
  compact({
    q: text(s, "q"),
    folder: text(s, "folder"),
    state: oneOf(s, "state", ["live", "current", "archived"] as const),
  }),
);
/** The IT Calendar; month, Calendar Category and view in the URL (FR-CAL-001). */
const calendarRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/calendar",
  validateSearch: (s: Record<string, unknown>) =>
    compact({
      month: text(s, "month"),
      category: text(s, "category"),
      view: oneOf(s, "view", ["month", "list"] as const),
    }),
  component: withPage(P.Calendar, function CalendarRoute() {
    const { month, category, view } = calendarRoute.useSearch();
    return <P.Calendar month={parseMonth(month)} category={category} view={view} />;
  }),
});
/** A section with tabs whose index opens its first tab. */
function section(
  path: "/service-status" | "/morning-checks" | "/users" | "/administration",
  layout: Page,
) {
  return createRoute({ getParentRoute: () => analystRoute, path, component: screen(layout) });
}

const statusRoute = section("/service-status", P.StatusLayout);
const statusIndex = createRoute({
  getParentRoute: () => statusRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/service-status/overview" });
  },
});
const statusOverview = createRoute({
  getParentRoute: () => statusRoute,
  path: "/overview",
  component: screen(P.StatusOverview),
});
const incidentsRoute = createRoute({
  getParentRoute: () => statusRoute,
  path: "/incidents",
  validateSearch: (s: Record<string, unknown>) => compact({ all: flag(s, "all") }),
  component: withPage(P.Incidents, function IncidentsRoute() {
    return <P.Incidents all={Boolean(incidentsRoute.useSearch().all)} />;
  }),
});
const newIncidentRoute = createRoute({
  getParentRoute: () => incidentsRoute,
  path: "new",
  component: withPage(P.Incident, function NewIncidentRoute() {
    return <P.Incident />;
  }),
});
const incidentRoute = createRoute({
  getParentRoute: () => incidentsRoute,
  path: "$id",
  component: withPage(P.Incident, function IncidentRoute() {
    const { id } = incidentRoute.useParams();
    return <P.Incident key={id} id={id} />;
  }),
});
const servicesRoute = createRoute({
  getParentRoute: () => statusRoute,
  path: "/services",
  component: screen(P.Services),
});

const checksRoute = section("/morning-checks", P.ChecksLayout);
const checksIndex = createRoute({
  getParentRoute: () => checksRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/morning-checks/today" });
  },
});
const roundRoute = createRoute({
  getParentRoute: () => checksRoute,
  path: "/today",
  validateSearch: (s: Record<string, unknown>) => {
    const date = text(s, "date");
    return compact({ date: isDayKey(date) ? date : undefined, mine: flag(s, "mine") });
  },
  component: withPage(P.Round, function RoundRoute() {
    const { date, mine } = roundRoute.useSearch();
    return <P.Round date={date} mine={Boolean(mine)} />;
  }),
});
const checksListRoute = createRoute({
  getParentRoute: () => checksRoute,
  path: "/checks",
  component: screen(P.Checks),
});
const usersRoute = section("/users", P.UsersLayout);
const usersIndex = createRoute({
  getParentRoute: () => usersRoute,
  path: "/",
  beforeLoad: async ({ context }) => {
    const session = await context.queryClient.ensureQueryData(analystSessionQuery);
    const analysts =
      session && (session.is_admin || session.capabilities.includes("analysts.manage"));
    throw redirect({ to: analysts ? "/users/analysts" : "/users/requesters" });
  },
});
const peopleSearch = (s: Record<string, unknown>) =>
  compact({ q: text(s, "q"), inactive: flag(s, "inactive") });
function peopleRoutes<B extends "/analysts" | "/requesters">(base: B, List: Page, Detail: Page) {
  const listRoute = createRoute({
    getParentRoute: () => usersRoute,
    path: base,
    validateSearch: peopleSearch,
    component: withPage(List, function PeopleRoute() {
      return <List search={listRoute.useSearch()} />;
    }),
  });
  const newRoute = createRoute({
    getParentRoute: () => listRoute,
    path: "new",
    component: withPage(Detail, function NewPersonRoute() {
      return <Detail />;
    }),
  });
  const byIdRoute = createRoute({
    getParentRoute: () => listRoute,
    path: "$id",
    component: withPage(Detail, function PersonRoute() {
      const { id } = byIdRoute.useParams() as { id: string };
      return <Detail key={id} id={id} />;
    }),
  });
  return listRoute.addChildren([newRoute, byIdRoute]);
}
const adminRoute = section("/administration", P.AdminLayout);
const adminPage = <Path extends string>(path: Path, page: Page) =>
  createRoute({ getParentRoute: () => adminRoute, path, component: screen(page) });
const referenceRoute = <K extends string>(kind: K) =>
  adminPage(
    `/${kind}` as `/${K}`,
    Object.assign(
      // biome-ignore lint/correctness/noComponentHookFactories: route factories run once, at module load
      function ReferenceRoute() {
        return <P.Reference kind={kind} />;
      },
      { preload: P.Reference.preload },
    ),
  );
const mailboxesRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: "/mailboxes",
  validateSearch: (s: Record<string, unknown>) =>
    compact({ mailbox_consent: text(s, "mailbox_consent"), mailbox: text(s, "mailbox") }),
  component: withPage(P.Mailboxes, function MailboxesRoute() {
    return <P.Mailboxes search={mailboxesRoute.useSearch()} />;
  }),
});
const logRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: "/log",
  validateSearch: (s: Record<string, unknown>) =>
    compact({ level: oneOf(s, "level", ["warn", "error"] as const), q: text(s, "q") }),
  component: withPage(P.SystemLog, function LogRoute() {
    return <P.SystemLog search={logRoute.useSearch()} />;
  }),
});
const adminRoutes = [
  createRoute({ getParentRoute: () => adminRoute, path: "/", component: screen(P.AdminIndex) }),
  adminPage("/settings", P.Settings),
  adminPage("/modules", P.Modules),
  adminPage("/jobs", P.Jobs),
  logRoute,
  referenceRoute("ticket-statuses"),
  referenceRoute("ticket-priorities"),
  referenceRoute("ticket-types"),
  referenceRoute("ticket-origins"),
  referenceRoute("resolution-codes"),
  referenceRoute("asset-types"),
  referenceRoute("asset-statuses"),
  referenceRoute("asset-locations"),
  referenceRoute("supplier-types"),
  referenceRoute("supplier-statuses"),
  referenceRoute("contract-statuses"),
  referenceRoute("payment-schedules"),
  referenceRoute("calendar-categories"),
  adminPage("/ticket-categories", P.Categories),
  mailboxesRoute,
  adminPage("/suggested-reply-instructions", P.SuggestedReplyInstructions),
  adminPage("/companies", P.Companies),
  adminPage("/teams", P.Teams),
  adminPage("/departments", P.Departments),
  adminPage("/cost-centres", P.CostCentres),
  adminPage("/roles", P.Roles),
];
/* Self-Service Portal (FR-PRT-001…007): plain pages for occasional users. */
const portalSignInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/sign-in",
  validateSearch: redirectSearch,
  component: withPage(P.PortalSignIn, function PortalSignInRoute() {
    return <P.PortalSignIn redirect={portalSignInRoute.useSearch().redirect} />;
  }),
});
const portalResetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/reset-password",
  component: screen(P.PortalReset),
});
/** The link in Portal Invitation, reset and Self-Registration emails. */
const portalSetPasswordRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/set-password",
  validateSearch: tokenSearch,
  component: withPage(P.PortalSetPassword, function PortalSetPasswordRoute() {
    return <P.PortalSetPassword token={portalSetPasswordRoute.useSearch().token} />;
  }),
});
const portalRegisterRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/register",
  component: screen(P.PortalRegister),
});

const portalRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal",
  beforeLoad: async ({ context, location }) => {
    if (!(await context.queryClient.ensureQueryData(portalSessionQuery))) {
      throw redirect({ to: "/portal/sign-in", search: { redirect: location.href } });
    }
  },
  component: screen(P.PortalLayout),
});
const portalChild = <Path extends string>(path: Path, page: Page) =>
  createRoute({ getParentRoute: () => portalRoute, path, component: screen(page) });
const portalTicketsRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/tickets",
  validateSearch: (s: Record<string, unknown>) =>
    compact({ show: oneOf(s, "show", ["open", "closed", "managed"] as const) }),
  component: withPage(P.PortalTickets, function PortalTicketsRoute() {
    return <P.PortalTickets show={portalTicketsRoute.useSearch().show ?? "open"} />;
  }),
});
const portalNewTicketRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/tickets/new",
  validateSearch: (s: Record<string, unknown>) => compact({ category: text(s, "category") }),
  component: withPage(P.PortalNewTicket, function PortalNewTicketRoute() {
    return <P.PortalNewTicket category={portalNewTicketRoute.useSearch().category} />;
  }),
});
const portalTicketRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/tickets/$id",
  component: withPage(P.PortalTicket, function PortalTicketRoute() {
    const { id } = portalTicketRoute.useParams();
    return <P.PortalTicket key={id} id={id} />;
  }),
});
const portalKnowledgeRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/knowledge",
  validateSearch: (s: Record<string, unknown>) => compact({ q: text(s, "q") }),
  component: withPage(P.PortalKnowledge, function PortalKnowledgeRoute() {
    return <P.PortalKnowledge q={portalKnowledgeRoute.useSearch().q} />;
  }),
});
const portalArticleRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/knowledge/$id",
  component: withPage(P.PortalArticle, function PortalArticleRoute() {
    const { id } = portalArticleRoute.useParams();
    return <P.PortalArticle key={id} id={id} />;
  }),
});

export const routeTree = rootRoute.addChildren([
  signInRoute,
  resetRoute,
  setupRoute,
  twoFactorRoute,
  analystRoute.addChildren([
    homeRoute,
    notificationsRoute,
    accountRoute,
    ticketsRoute.addChildren([newTicketRoute, ticketRoute]),
    tasksRoutes,
    assetsRoutes,
    contractsRoutes,
    suppliersRoutes,
    knowledgeRoutes,
    calendarRoute,
    statusRoute.addChildren([
      statusIndex,
      statusOverview,
      incidentsRoute.addChildren([newIncidentRoute, incidentRoute]),
      servicesRoute,
    ]),
    checksRoute.addChildren([checksIndex, roundRoute, checksListRoute]),
    usersRoute.addChildren([
      usersIndex,
      peopleRoutes("/analysts", P.Analysts, P.Analyst),
      peopleRoutes("/requesters", P.Requesters, P.Requester),
    ]),
    adminRoute.addChildren(adminRoutes),
  ]),
  portalSignInRoute,
  portalResetRoute,
  portalSetPasswordRoute,
  portalRegisterRoute,
  portalRoute.addChildren([
    portalChild("/", P.PortalHome),
    portalTicketsRoute,
    portalNewTicketRoute,
    portalTicketRoute,
    portalKnowledgeRoute,
    portalArticleRoute,
    portalChild("/service-status", P.PortalStatus),
    portalChild("/notifications", P.PortalNotifications),
  ]),
]);

/** Server state lives in TanStack Query; no global client store (ADR-0010). */
export function makeQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { staleTime: 30_000, retry: 1, refetchOnWindowFocus: true } },
  });
}

/** Creates the router; tests pass their own history and query client. */
export function makeRouter(
  queryClient: QueryClient,
  options: Partial<Omit<Parameters<typeof createRouter>[0], "routeTree" | "context">> = {},
) {
  return createRouter({
    routeTree,
    context: { queryClient },
    defaultPreload: "intent",
    defaultNotFoundComponent: NotFound,
    notFoundMode: "root",
    ...options,
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof makeRouter>;
  }
}
