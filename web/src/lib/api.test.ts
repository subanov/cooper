import { mockApi } from "@/test/render";
import { api, areaOf, setCsrfToken } from "./api";

describe("the API client", () => {
  afterEach(() => {
    setCsrfToken("analyst", undefined);
    setCsrfToken("portal", undefined);
  });

  it("sends the CSRF token of the right session on unsafe requests only", async () => {
    const calls = mockApi({
      "/api/v1/tickets/1/notes": { status: 201, body: {} },
      "/api/v1/portal/tickets": { status: 201, body: {} },
      "/api/v1/session": { status: 200, body: {} },
    });
    setCsrfToken("analyst", "analyst-token");
    setCsrfToken("portal", "portal-token");
    await api.POST("/api/v1/tickets/{id}/notes", {
      params: { path: { id: "1" } },
      body: { body: "x" },
    });
    await api.POST("/api/v1/portal/tickets", { body: { subject: "x" } });
    await api.GET("/api/v1/session");
    expect(calls[0]?.headers.get("X-CSRF-Token")).toBe("analyst-token");
    expect(calls[1]?.headers.get("X-CSRF-Token")).toBe("portal-token");
    expect(calls[2]?.headers.get("X-CSRF-Token")).toBeNull();
  });

  it("never stores tokens in localStorage", async () => {
    setCsrfToken("analyst", "secret");
    expect(JSON.stringify({ ...window.localStorage })).not.toContain("secret");
  });

  it("tells the Portal and Analyst areas apart", () => {
    expect(areaOf("/api/v1/portal/session")).toBe("portal");
    expect(areaOf("/api/v1/tickets")).toBe("analyst");
  });
});
