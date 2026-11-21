import { QueryClient } from "@tanstack/react-query";
import { applyLiveEvent } from "./live";

describe("Live Updates", () => {
  it("marks the ticket list and that ticket stale when a ticket changes", async () => {
    const client = new QueryClient();
    const spy = vi.spyOn(client, "invalidateQueries");
    applyLiveEvent(client, "invalidate", JSON.stringify({ topic: "ticket", id: "t1" }));
    const keys = spy.mock.calls.map((c) => c[0]?.queryKey);
    expect(keys).toContainEqual(["tickets", "list"]);
    expect(keys).toContainEqual(["tickets", "detail", "t1"]);
  });

  it("refreshes presence only for presence events, and everything on resync", () => {
    const client = new QueryClient();
    const spy = vi.spyOn(client, "invalidateQueries");
    applyLiveEvent(client, "invalidate", JSON.stringify({ topic: "ticket_presence", id: "t1" }));
    expect(spy).toHaveBeenCalledWith({ queryKey: ["tickets", "presence", "t1"] });
    applyLiveEvent(client, "resync", "{}");
    expect(spy).toHaveBeenLastCalledWith();
  });

  it("ignores malformed and unknown events", () => {
    const client = new QueryClient();
    const spy = vi.spyOn(client, "invalidateQueries");
    applyLiveEvent(client, "invalidate", "not json");
    applyLiveEvent(client, "ping", "{}");
    expect(spy).not.toHaveBeenCalled();
  });
});
