import { QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  redirect,
} from "@tanstack/react-router";
import type { LookupKind } from "./features/administration/resources";
import { parseMonth } from "./features/calendar/month";
import type { ContractLookupKind } from "./features/contracts/api";
import { analystSessionQuery, portalSessionQuery } from "./features/session/queries";
import { parseTicketSearch } from "./features/tickets/views";
import { AnalystLayout } from "./routes/analyst/AnalystLayout";
import { AdministrationLayout } from "./routes/analyst/administration/AdministrationLayout";
import { JobsPage } from "./routes/analyst/administration/JobsPage";
import { ModulesPage } from "./routes/analyst/administration/ModulesPage";
import { AssetLookupsPage } from "./routes/analyst/administration/resources/AssetLookupsPage";
import { CalendarCategoriesPage } from "./routes/analyst/administration/resources/CalendarCategoriesPage";
import { CategoriesPage } from "./routes/analyst/administration/resources/CategoriesPage";
import { CompaniesPage } from "./routes/analyst/administration/resources/CompaniesPage";
import { ContractLookupPage } from "./routes/analyst/administration/resources/ContractLookupPage";
import { CostCentresPage } from "./routes/analyst/administration/resources/CostCentresPage";
import { DepartmentsPage } from "./routes/analyst/administration/resources/DepartmentsPage";
import { LookupAdminPage } from "./routes/analyst/administration/resources/LookupAdminPage";
import { MailboxesPage } from "./routes/analyst/administration/resources/MailboxesPage";
import { RolesPage } from "./routes/analyst/administration/resources/RolesPage";
import { TeamsPage } from "./routes/analyst/administration/resources/TeamsPage";
import { SettingsPage } from "./routes/analyst/administration/SettingsPage";
import { SystemLogPage } from "./routes/analyst/administration/SystemLogPage";
import { AssetEditor } from "./routes/analyst/assets/AssetEditor";
import { AssetsPage } from "./routes/analyst/assets/AssetsPage";
import { ComingSoon } from "./routes/analyst/ComingSoon";
import { CalendarPage } from "./routes/analyst/calendar/CalendarPage";
import { ContractEditor } from "./routes/analyst/contracts/ContractEditor";
import { ContractsPage } from "./routes/analyst/contracts/ContractsPage";
import { SupplierEditor } from "./routes/analyst/contracts/SupplierEditor";
import { SuppliersPage } from "./routes/analyst/contracts/SuppliersPage";
import { Home } from "./routes/analyst/Home";
import { ArticlePage } from "./routes/analyst/knowledge/ArticlePage";
import { KnowledgePage } from "./routes/analyst/knowledge/KnowledgePage";
import { ChecksPage } from "./routes/analyst/morning-checks/ChecksPage";
import { MorningChecksLayout } from "./routes/analyst/morning-checks/MorningChecksLayout";
import { TodayPage } from "./routes/analyst/morning-checks/TodayPage";
import { IncidentEditor } from "./routes/analyst/service-status/IncidentEditor";
import { IncidentsPage } from "./routes/analyst/service-status/IncidentsPage";
import { OverviewPage } from "./routes/analyst/service-status/OverviewPage";
import { ServiceStatusLayout } from "./routes/analyst/service-status/ServiceStatusLayout";
import { ServicesPage } from "./routes/analyst/service-status/ServicesPage";
import { TaskEditor } from "./routes/analyst/tasks/TaskEditor";
import { TasksPage } from "./routes/analyst/tasks/TasksPage";
import { NewTicket } from "./routes/analyst/tickets/NewTicket";
import { TicketDetail } from "./routes/analyst/tickets/TicketDetail";
import { TicketsPage } from "./routes/analyst/tickets/TicketsPage";
import { AnalystEditor } from "./routes/analyst/users/AnalystEditor";
import { AnalystsPage } from "./routes/analyst/users/AnalystsPage";
import { RequesterEditor } from "./routes/analyst/users/RequesterEditor";
import { RequestersPage } from "./routes/analyst/users/RequestersPage";
import { UsersLayout } from "./routes/analyst/users/UsersLayout";
import { ResetPasswordPage } from "./routes/auth/ResetPassword";
import { SignInPage } from "./routes/auth/SignIn";
import { TwoFactorSetupPage } from "./routes/auth/TwoFactorSetup";
import { NotFound } from "./routes/NotFound";
import { PortalHome } from "./routes/portal/PortalHome";
import { PortalLayout } from "./routes/portal/PortalLayout";
import { PortalResetPage, PortalSetPasswordPage } from "./routes/portal/PortalPassword";
import { PortalRegisterPage } from "./routes/portal/PortalRegister";
import { PortalSignInPage } from "./routes/portal/PortalSignIn";

/** What every route can use: the query cache, for loading data before rendering. */
export interface RouterContext {
  queryClient: QueryClient;
}

/**
 * Two route trees share one app (ADR-0010): the Analyst console at `/` and the
 * Self-Service Portal at `/portal`. List filters, sort and selection will live in the URL.
 */
const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: Outlet,
  notFoundComponent: NotFound,
});

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  validateSearch: (search: Record<string, unknown>): { redirect?: string } =>
    typeof search.redirect === "string" ? { redirect: search.redirect } : {},
  component: function SignInRoute() {
    const { redirect: target } = signInRoute.useSearch();
    return <SignInPage redirect={target} />;
  },
});

const resetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/reset-password",
  validateSearch: (search: Record<string, unknown>): { token?: string } =>
    typeof search.token === "string" ? { token: search.token } : {},
  component: function ResetRoute() {
    const { token } = resetRoute.useSearch();
    return <ResetPasswordPage token={token} />;
  },
});

const twoFactorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/two-factor/setup",
  beforeLoad: async ({ context }) => {
    const session = await context.queryClient.ensureQueryData(analystSessionQuery);
    if (!session) {
      throw redirect({ to: "/sign-in" });
    }
  },
  component: TwoFactorSetupPage,
});

/** Signed-in Analyst screens: sign-in and required two-factor setup come first. */
const analystRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "analyst",
  beforeLoad: async ({ context, location }) => {
    const session = await context.queryClient.ensureQueryData(analystSessionQuery);
    if (!session) {
      throw redirect({ to: "/sign-in", search: { redirect: location.href } });
    }
    if (session.totp_enrollment_required) {
      throw redirect({ to: "/two-factor/setup" });
    }
  },
  component: AnalystLayout,
});

const homeRoute = createRoute({ getParentRoute: () => analystRoute, path: "/", component: Home });

/** The ticket list; view, search and sort are in the URL (FR-TKT-040). */
const ticketsRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/tickets",
  validateSearch: parseTicketSearch,
  component: TicketsPage,
});

/** A ticket opened beside the list (split pane). */
const ticketRoute = createRoute({
  getParentRoute: () => ticketsRoute,
  path: "$ticketId",
  component: function TicketRoute() {
    const { ticketId } = ticketRoute.useParams();
    return <TicketDetail key={ticketId} ticketId={ticketId} />;
  },
});

/** The New ticket form, in the split pane beside the list. */
const newTicketRoute = createRoute({
  getParentRoute: () => ticketsRoute,
  path: "new",
  component: NewTicket,
});

const tasksRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/tasks",
  component: TasksPage,
});
const newTaskRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/tasks/new",
  component: () => <TaskEditor />,
});
const taskByIdRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/tasks/$id",
  component: function TaskRoute() {
    const { id } = taskByIdRoute.useParams();
    return <TaskEditor key={id} id={id} />;
  },
});

const assetsRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/assets",
  component: AssetsPage,
});
const newAssetRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/assets/new",
  component: () => <AssetEditor />,
});
const assetByIdRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/assets/$id",
  component: function AssetRoute() {
    const { id } = assetByIdRoute.useParams();
    return <AssetEditor key={id} id={id} />;
  },
});
const contractsRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/contracts",
  component: ContractsPage,
});
const newContractRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/contracts/new",
  component: () => <ContractEditor />,
});
const contractByIdRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/contracts/$id",
  component: function ContractRoute() {
    const { id } = contractByIdRoute.useParams();
    return <ContractEditor key={id} id={id} />;
  },
});
const suppliersRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/suppliers",
  component: SuppliersPage,
});
const newSupplierRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/suppliers/new",
  component: () => <SupplierEditor />,
});
const supplierByIdRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/suppliers/$id",
  component: function SupplierRoute() {
    const { id } = supplierByIdRoute.useParams();
    return <SupplierEditor key={id} id={id} />;
  },
});
/** The IT Calendar; the month and Category filter are in the URL (FR-CAL-001). */
const calendarRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/calendar",
  validateSearch: (search: Record<string, unknown>): { month?: string; category?: string } => ({
    ...(typeof search.month === "string" ? { month: search.month } : {}),
    ...(typeof search.category === "string" && search.category
      ? { category: search.category }
      : {}),
  }),
  component: function CalendarRoute() {
    const { month, category } = calendarRoute.useSearch();
    return <CalendarPage month={parseMonth(month)} {...(category ? { category } : {})} />;
  },
});

const knowledgeRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/knowledge",
  component: KnowledgePage,
});
const newArticleRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/knowledge/new",
  component: () => <ArticlePage />,
});
const articleByIdRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/knowledge/$id",
  component: function ArticleRoute() {
    const { id } = articleByIdRoute.useParams();
    return <ArticlePage key={id} id={id} />;
  },
});

const serviceStatusRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/service-status",
  component: ServiceStatusLayout,
});
const serviceStatusIndexRoute = createRoute({
  getParentRoute: () => serviceStatusRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/service-status/overview" });
  },
});
const statusOverviewRoute = createRoute({
  getParentRoute: () => serviceStatusRoute,
  path: "/overview",
  component: OverviewPage,
});
const statusIncidentsRoute = createRoute({
  getParentRoute: () => serviceStatusRoute,
  path: "/incidents",
  component: IncidentsPage,
});
const statusNewIncidentRoute = createRoute({
  getParentRoute: () => serviceStatusRoute,
  path: "/incidents/new",
  component: () => <IncidentEditor />,
});
const statusIncidentRoute = createRoute({
  getParentRoute: () => serviceStatusRoute,
  path: "/incidents/$id",
  component: function IncidentRoute() {
    const { id } = statusIncidentRoute.useParams();
    return <IncidentEditor key={id} id={id} />;
  },
});
const statusServicesRoute = createRoute({
  getParentRoute: () => serviceStatusRoute,
  path: "/services",
  component: ServicesPage,
});

const morningChecksRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/morning-checks",
  component: MorningChecksLayout,
});
const morningChecksIndexRoute = createRoute({
  getParentRoute: () => morningChecksRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/morning-checks/today" });
  },
});
const checksTodayRoute = createRoute({
  getParentRoute: () => morningChecksRoute,
  path: "/today",
  component: TodayPage,
});
const checksListRoute = createRoute({
  getParentRoute: () => morningChecksRoute,
  path: "/checks",
  component: ChecksPage,
});

const usersRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/users",
  component: UsersLayout,
});
const usersIndexRoute = createRoute({
  getParentRoute: () => usersRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/users/analysts" });
  },
});
const analystsRoute = createRoute({
  getParentRoute: () => usersRoute,
  path: "/analysts",
  component: AnalystsPage,
});
const newAnalystRoute = createRoute({
  getParentRoute: () => usersRoute,
  path: "/analysts/new",
  component: () => <AnalystEditor />,
});
const analystRouteById = createRoute({
  getParentRoute: () => usersRoute,
  path: "/analysts/$id",
  component: function AnalystRoute() {
    const { id } = analystRouteById.useParams();
    return <AnalystEditor key={id} id={id} />;
  },
});
const requestersRoute = createRoute({
  getParentRoute: () => usersRoute,
  path: "/requesters",
  component: RequestersPage,
});
const newRequesterRoute = createRoute({
  getParentRoute: () => usersRoute,
  path: "/requesters/new",
  component: () => <RequesterEditor />,
});
const requesterRouteById = createRoute({
  getParentRoute: () => usersRoute,
  path: "/requesters/$id",
  component: function RequesterRoute() {
    const { id } = requesterRouteById.useParams();
    return <RequesterEditor key={id} id={id} />;
  },
});

const administrationRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/administration",
  component: AdministrationLayout,
});

const administrationIndexRoute = createRoute({
  getParentRoute: () => administrationRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/administration/settings" });
  },
});
const administrationSettingsRoute = createRoute({
  getParentRoute: () => administrationRoute,
  path: "/settings",
  component: SettingsPage,
});
const administrationModulesRoute = createRoute({
  getParentRoute: () => administrationRoute,
  path: "/modules",
  component: ModulesPage,
});
const administrationJobsRoute = createRoute({
  getParentRoute: () => administrationRoute,
  path: "/jobs",
  component: JobsPage,
});
const administrationLogRoute = createRoute({
  getParentRoute: () => administrationRoute,
  path: "/log",
  component: SystemLogPage,
});

/** An Administration page for ticket reference data or an organisation list. */
function adminPage<P extends string>(path: P, component: () => React.JSX.Element) {
  return createRoute({ getParentRoute: () => administrationRoute, path, component });
}
const lookupPage = (kind: LookupKind) => () => <LookupAdminPage kind={kind} />;
const contractLookupPage = (kind: ContractLookupKind) => () => <ContractLookupPage kind={kind} />;
const adminReferenceRoutes = [
  adminPage("/ticket-statuses", lookupPage("ticket-statuses")),
  adminPage("/ticket-priorities", lookupPage("ticket-priorities")),
  adminPage("/ticket-types", lookupPage("ticket-types")),
  adminPage("/ticket-origins", lookupPage("ticket-origins")),
  adminPage("/resolution-codes", lookupPage("resolution-codes")),
  adminPage("/ticket-categories", CategoriesPage),
  adminPage("/mailboxes", MailboxesPage),
  adminPage("/companies", CompaniesPage),
  adminPage("/teams", TeamsPage),
  adminPage("/departments", DepartmentsPage),
  adminPage("/cost-centres", CostCentresPage),
  adminPage("/roles", RolesPage),
  adminPage("/asset-types", () => <AssetLookupsPage kind="asset-types" />),
  adminPage("/asset-statuses", () => <AssetLookupsPage kind="asset-statuses" />),
  adminPage("/asset-locations", () => <AssetLookupsPage kind="asset-locations" />),
  adminPage("/supplier-types", contractLookupPage("supplier-types")),
  adminPage("/supplier-statuses", contractLookupPage("supplier-statuses")),
  adminPage("/contract-statuses", contractLookupPage("contract-statuses")),
  adminPage("/payment-schedules", contractLookupPage("payment-schedules")),
  adminPage("/calendar-categories", CalendarCategoriesPage),
] as const;

const portalSignInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/sign-in",
  validateSearch: (search: Record<string, unknown>): { redirect?: string } =>
    typeof search.redirect === "string" ? { redirect: search.redirect } : {},
  component: function PortalSignInRoute() {
    const { redirect: target } = portalSignInRoute.useSearch();
    return <PortalSignInPage redirect={target} />;
  },
});

const portalResetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/reset-password",
  component: PortalResetPage,
});

/** The link in Portal Invitation, reset and Self-Registration emails. */
const portalSetPasswordRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/set-password",
  validateSearch: (search: Record<string, unknown>): { token?: string } =>
    typeof search.token === "string" ? { token: search.token } : {},
  component: function PortalSetPasswordRoute() {
    const { token } = portalSetPasswordRoute.useSearch();
    return <PortalSetPasswordPage token={token} />;
  },
});

const portalRegisterRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/register",
  component: PortalRegisterPage,
});

/** Signed-in Portal screens (Requesters, FR-PRT-001). */
const portalRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal",
  beforeLoad: async ({ context, location }) => {
    const session = await context.queryClient.ensureQueryData(portalSessionQuery);
    if (!session) {
      throw redirect({ to: "/portal/sign-in", search: { redirect: location.href } });
    }
  },
  component: PortalLayout,
});

const portalHomeRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/",
  component: PortalHome,
});

const portalTicketsRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/tickets",
  component: () => <ComingSoon section="portalNav.tickets" />,
});

export const routeTree = rootRoute.addChildren([
  signInRoute,
  resetRoute,
  twoFactorRoute,
  analystRoute.addChildren([
    homeRoute,
    ticketsRoute.addChildren([newTicketRoute, ticketRoute]),
    tasksRoute,
    newTaskRoute,
    taskByIdRoute,
    assetsRoute,
    newAssetRoute,
    assetByIdRoute,
    contractsRoute,
    newContractRoute,
    contractByIdRoute,
    suppliersRoute,
    newSupplierRoute,
    supplierByIdRoute,
    calendarRoute,
    knowledgeRoute,
    newArticleRoute,
    articleByIdRoute,
    serviceStatusRoute.addChildren([
      serviceStatusIndexRoute,
      statusOverviewRoute,
      statusIncidentsRoute,
      statusNewIncidentRoute,
      statusIncidentRoute,
      statusServicesRoute,
    ]),
    morningChecksRoute.addChildren([morningChecksIndexRoute, checksTodayRoute, checksListRoute]),
    usersRoute.addChildren([
      usersIndexRoute,
      analystsRoute,
      newAnalystRoute,
      analystRouteById,
      requestersRoute,
      newRequesterRoute,
      requesterRouteById,
    ]),
    administrationRoute.addChildren([
      administrationIndexRoute,
      administrationSettingsRoute,
      administrationModulesRoute,
      administrationJobsRoute,
      administrationLogRoute,
      ...adminReferenceRoutes,
    ]),
  ]),
  portalSignInRoute,
  portalResetRoute,
  portalSetPasswordRoute,
  portalRegisterRoute,
  portalRoute.addChildren([portalHomeRoute, portalTicketsRoute]),
]);

/** Server state: no global client store (ADR-0010). */
export function makeQueryClient() {
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
    ...options,
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof makeRouter>;
  }
}
