import { describe, expect, it } from "vitest";
import { csrfMiddleware, setCsrfToken } from "./api";

async function headerFor(method: string, path: string) {
  const request = new Request(`http://localhost${path}`, { method });
  const onRequest = csrfMiddleware.onRequest;
  if (!onRequest) {
    throw new Error("missing onRequest");
  }
  const out = (await onRequest({
    request,
    schemaPath: path,
    params: {},
    options: {} as never,
    id: "1",
  })) as Request;
  return out.headers.get("X-CSRF-Token");
}

describe("CSRF middleware", () => {
  it("sends each area's token on unsafe requests only", async () => {
    setCsrfToken("analyst", "analyst-token");
    setCsrfToken("portal", "portal-token");
    expect(await headerFor("GET", "/api/v1/tickets")).toBeNull();
    expect(await headerFor("POST", "/api/v1/tickets")).toBe("analyst-token");
    expect(await headerFor("DELETE", "/api/v1/portal/session")).toBe("portal-token");
    setCsrfToken("portal", undefined);
    expect(await headerFor("POST", "/api/v1/portal/tickets")).toBeNull();
  });
});
