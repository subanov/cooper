import { QueryClient } from "@tanstack/react-query";
import {
  type AsyncRouteComponent,
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

/* Screens load on demand, so the first download holds only the frame. */
const lazy = <M extends Record<string, unknown>, K extends keyof M & string>(
  load: () => Promise<M>,
  name: K,
) => lazyRouteComponent(load as () => Promise<Record<string, unknown>>, name);

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
  Calendar: lazy(() => import("./features/calendar/CalendarPage"), "CalendarPage"),
  Knowledge: lazy(() => import("./features/knowledge/KnowledgePage"), "KnowledgePage"),
  Article: lazy(() => import("./features/knowledge/ArticlePage"), "ArticlePage"),
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

/** A route component that renders a lazy page with route props; preloaded with the route. */
function withPage(
  page: AsyncRouteComponent<unknown>,
  component: () => React.JSX.Element,
): RouteComponent {
  const { preload } = page;
  return preload ? Object.assign(component, { preload }) : component;
}

/* biome-ignore-start lint/suspicious/noExplicitAny: lazy pages take their props from the route wrappers below */
type AnyPage = React.ComponentType<any>;
/* biome-ignore-end lint/suspicious/noExplicitAny: end */
const pageOf = (page: AsyncRouteComponent<unknown>) => page as unknown as AnyPage;

/** What every route can use: the query cache, for loading data before rendering. */
export interface RouterContext {
  queryClient: QueryClient;
}

/**
 * Two route trees in one app (ADR-0010): the Analyst console at `/` and the
 * Self-Service Portal at `/portal`. Paths use Vocabulary terms.
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
    const Page = pageOf(P.ResetPassword);
    return <Page token={resetRoute.useSearch().token} />;
  }),
});
const setupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/setup",
  component: P.Setup,
});
const twoFactorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/two-factor/setup",
  beforeLoad: async ({ context }) => {
    if (!(await context.queryClient.ensureQueryData(analystSessionQuery))) {
      throw redirect({ to: "/sign-in" });
    }
  },
  component: P.TwoFactor,
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

const child = <Path extends string>(path: Path, component: RouteComponent) =>
  createRoute({ getParentRoute: () => analystRoute, path, component });

const homeRoute = child("/", P.Home);
const notificationsRoute = child("/notifications", P.Notifications);
const accountRoute = child("/account", P.Account);

/** Tickets: the list with a split pane (FR-TKT-040); view, search and sort in the URL. */
const ticketsRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/tickets",
  validateSearch: parseTicketSearch,
  component: withPage(P.Tickets, function TicketsRoute() {
    const Page = pageOf(P.Tickets);
    return <Page search={ticketsRoute.useSearch()} />;
  }),
});
const ticketRoute = createRoute({
  getParentRoute: () => ticketsRoute,
  path: "$ticketId",
  component: withPage(P.Ticket, function TicketRoute() {
    const Page = pageOf(P.Ticket);
    const { ticketId } = ticketRoute.useParams();
    return <Page key={ticketId} ticketId={ticketId} />;
  }),
});
const newTicketRoute = createRoute({
  getParentRoute: () => ticketsRoute,
  path: "new",
  /** `task`: the Task the new ticket is linked to once created (FR-TSK-004). */
  validateSearch: (s: Record<string, unknown>) => compact({ task: text(s, "task") }),
  component: withPage(P.NewTicket, function NewTicketRoute() {
    const Page = pageOf(P.NewTicket);
    return <Page taskId={newTicketRoute.useSearch().task} />;
  }),
});

/** A list screen with URL search state, plus its New and detail screens. */
function recordRoutes<
  B extends "/tasks" | "/assets" | "/contracts" | "/suppliers" | "/knowledge",
  S,
>(
  base: B,
  list: AsyncRouteComponent<unknown>,
  detail: AsyncRouteComponent<unknown>,
  validateSearch: (s: Record<string, unknown>) => S,
) {
  const listRoute = createRoute({
    getParentRoute: () => analystRoute,
    path: base,
    validateSearch,
    // biome-ignore lint/correctness/noComponentHookFactories: route factories run once, at module load
    component: withPage(list, function ListRoute() {
      const Page = pageOf(list);
      return <Page search={listRoute.useSearch()} />;
    }),
  });
  const newRoute = createRoute({
    getParentRoute: () => analystRoute,
    path: `${base}/new` as `${B}/new`,
    // biome-ignore lint/correctness/noComponentHookFactories: route factories run once, at module load
    component: withPage(detail, function NewRoute() {
      const Page = pageOf(detail);
      return <Page />;
    }),
  });
  const byIdRoute = createRoute({
    getParentRoute: () => analystRoute,
    path: `${base}/$id` as `${B}/$id`,
    // biome-ignore lint/correctness/noComponentHookFactories: route factories run once, at module load
    component: withPage(detail, function DetailRoute() {
      const Page = pageOf(detail);
      const { id } = byIdRoute.useParams() as { id: string };
      return <Page key={id} id={id} />;
    }),
  });
  return [listRoute, newRoute, byIdRoute] as const;
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

/** The IT Calendar; month and Calendar Category in the URL (FR-CAL-001). */
const calendarRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/calendar",
  validateSearch: (s: Record<string, unknown>) =>
    compact({ month: text(s, "month"), category: text(s, "category") }),
  component: withPage(P.Calendar, function CalendarRoute() {
    const Page = pageOf(P.Calendar);
    const { month, category } = calendarRoute.useSearch();
    return <Page month={parseMonth(month)} category={category} />;
  }),
});

/** A section with tabs and an index that opens its first tab. */
function section(
  path: "/service-status" | "/morning-checks" | "/users" | "/administration",
  layout: RouteComponent,
) {
  return createRoute({ getParentRoute: () => analystRoute, path, component: layout });
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
  component: P.StatusOverview,
});
const incidentsRoute = createRoute({
  getParentRoute: () => statusRoute,
  path: "/incidents",
  validateSearch: (s: Record<string, unknown>) => compact({ all: flag(s, "all") }),
  component: withPage(P.Incidents, function IncidentsRoute() {
    const Page = pageOf(P.Incidents);
    return <Page all={Boolean(incidentsRoute.useSearch().all)} />;
  }),
});
const newIncidentRoute = createRoute({
  getParentRoute: () => statusRoute,
  path: "/incidents/new",
  component: withPage(P.Incident, function NewIncidentRoute() {
    const Page = pageOf(P.Incident);
    return <Page />;
  }),
});
const incidentRoute = createRoute({
  getParentRoute: () => statusRoute,
  path: "/incidents/$id",
  component: withPage(P.Incident, function IncidentRoute() {
    const Page = pageOf(P.Incident);
    const { id } = incidentRoute.useParams();
    return <Page key={id} id={id} />;
  }),
});
const servicesRoute = createRoute({
  getParentRoute: () => statusRoute,
  path: "/services",
  component: P.Services,
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
    const Page = pageOf(P.Round);
    const { date, mine } = roundRoute.useSearch();
    return <Page date={date} mine={Boolean(mine)} />;
  }),
});
const checksListRoute = createRoute({
  getParentRoute: () => checksRoute,
  path: "/checks",
  component: P.Checks,
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
function peopleRoutes<B extends "/analysts" | "/requesters">(
  base: B,
  list: AsyncRouteComponent<unknown>,
  detail: AsyncRouteComponent<unknown>,
) {
  const listRoute = createRoute({
    getParentRoute: () => usersRoute,
    path: base,
    validateSearch: peopleSearch,
    // biome-ignore lint/correctness/noComponentHookFactories: route factories run once, at module load
    component: withPage(list, function PeopleRoute() {
      const Page = pageOf(list);
      return <Page search={listRoute.useSearch()} />;
    }),
  });
  const newRoute = createRoute({
    getParentRoute: () => usersRoute,
    path: `${base}/new` as `${B}/new`,
    // biome-ignore lint/correctness/noComponentHookFactories: route factories run once, at module load
    component: withPage(detail, function NewPersonRoute() {
      const Page = pageOf(detail);
      return <Page />;
    }),
  });
  const byIdRoute = createRoute({
    getParentRoute: () => usersRoute,
    path: `${base}/$id` as `${B}/$id`,
    // biome-ignore lint/correctness/noComponentHookFactories: route factories run once, at module load
    component: withPage(detail, function PersonRoute() {
      const Page = pageOf(detail);
      const { id } = byIdRoute.useParams() as { id: string };
      return <Page key={id} id={id} />;
    }),
  });
  return [listRoute, newRoute, byIdRoute] as const;
}
const analystsRoutes = peopleRoutes("/analysts", P.Analysts, P.Analyst);
const requestersRoutes = peopleRoutes("/requesters", P.Requesters, P.Requester);

const adminRoute = section("/administration", P.AdminLayout);
const adminIndex = createRoute({
  getParentRoute: () => adminRoute,
  path: "/",
  component: P.AdminIndex,
});
const adminPage = <Path extends string>(path: Path, component: RouteComponent) =>
  createRoute({ getParentRoute: () => adminRoute, path, component });
const referenceRoute = <K extends string>(kind: K) =>
  adminPage(
    `/${kind}` as `/${K}`,
    // biome-ignore lint/correctness/noComponentHookFactories: route factories run once, at module load
    withPage(P.Reference, function ReferenceRoute() {
      const Page = pageOf(P.Reference);
      return <Page kind={kind} />;
    }),
  );
const mailboxesRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: "/mailboxes",
  validateSearch: (s: Record<string, unknown>) =>
    compact({ mailbox_consent: text(s, "mailbox_consent"), mailbox: text(s, "mailbox") }),
  component: withPage(P.Mailboxes, function MailboxesRoute() {
    const Page = pageOf(P.Mailboxes);
    return <Page search={mailboxesRoute.useSearch()} />;
  }),
});
const logRoute = createRoute({
  getParentRoute: () => adminRoute,
  path: "/log",
  validateSearch: (s: Record<string, unknown>) =>
    compact({ level: oneOf(s, "level", ["warn", "error"] as const), q: text(s, "q") }),
  component: withPage(P.SystemLog, function LogRoute() {
    const Page = pageOf(P.SystemLog);
    return <Page search={logRoute.useSearch()} />;
  }),
});
const adminRoutes = [
  adminIndex,
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
  adminPage("/companies", P.Companies),
  adminPage("/teams", P.Teams),
  adminPage("/departments", P.Departments),
  adminPage("/cost-centres", P.CostCentres),
  adminPage("/roles", P.Roles),
];

/* Self-Service Portal (FR-PRT-001…007). */
const portalSignInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/sign-in",
  validateSearch: redirectSearch,
  component: withPage(P.PortalSignIn, function PortalSignInRoute() {
    const Page = pageOf(P.PortalSignIn);
    return <Page redirect={portalSignInRoute.useSearch().redirect} />;
  }),
});
const portalResetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/reset-password",
  component: P.PortalReset,
});
/** The link in Portal Invitation, reset and Self-Registration emails. */
const portalSetPasswordRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/set-password",
  validateSearch: tokenSearch,
  component: withPage(P.PortalSetPassword, function PortalSetPasswordRoute() {
    const Page = pageOf(P.PortalSetPassword);
    return <Page token={portalSetPasswordRoute.useSearch().token} />;
  }),
});
const portalRegisterRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/register",
  component: P.PortalRegister,
});

const portalRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal",
  beforeLoad: async ({ context, location }) => {
    if (!(await context.queryClient.ensureQueryData(portalSessionQuery))) {
      throw redirect({ to: "/portal/sign-in", search: { redirect: location.href } });
    }
  },
  component: P.PortalLayout,
});
const portalChild = <Path extends string>(path: Path, component: RouteComponent) =>
  createRoute({ getParentRoute: () => portalRoute, path, component });
const portalTicketsRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/tickets",
  validateSearch: (s: Record<string, unknown>) =>
    compact({ show: oneOf(s, "show", ["open", "closed", "managed"] as const) }),
  component: withPage(P.PortalTickets, function PortalTicketsRoute() {
    const Page = pageOf(P.PortalTickets);
    return <Page show={portalTicketsRoute.useSearch().show ?? "open"} />;
  }),
});
const portalNewTicketRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/tickets/new",
  validateSearch: (s: Record<string, unknown>) => compact({ category: text(s, "category") }),
  component: withPage(P.PortalNewTicket, function PortalNewTicketRoute() {
    const Page = pageOf(P.PortalNewTicket);
    return <Page category={portalNewTicketRoute.useSearch().category} />;
  }),
});
const portalTicketRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/tickets/$id",
  component: withPage(P.PortalTicket, function PortalTicketRoute() {
    const Page = pageOf(P.PortalTicket);
    const { id } = portalTicketRoute.useParams();
    return <Page key={id} id={id} />;
  }),
});
const portalKnowledgeRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/knowledge",
  validateSearch: (s: Record<string, unknown>) => compact({ q: text(s, "q") }),
  component: withPage(P.PortalKnowledge, function PortalKnowledgeRoute() {
    const Page = pageOf(P.PortalKnowledge);
    return <Page q={portalKnowledgeRoute.useSearch().q} />;
  }),
});
const portalArticleRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/knowledge/$id",
  component: withPage(P.PortalArticle, function PortalArticleRoute() {
    const Page = pageOf(P.PortalArticle);
    const { id } = portalArticleRoute.useParams();
    return <Page key={id} id={id} />;
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
    ...tasksRoutes,
    ...assetsRoutes,
    ...contractsRoutes,
    ...suppliersRoutes,
    ...knowledgeRoutes,
    calendarRoute,
    statusRoute.addChildren([
      statusIndex,
      statusOverview,
      incidentsRoute,
      newIncidentRoute,
      incidentRoute,
      servicesRoute,
    ]),
    checksRoute.addChildren([checksIndex, roundRoute, checksListRoute]),
    usersRoute.addChildren([usersIndex, ...analystsRoutes, ...requestersRoutes]),
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
