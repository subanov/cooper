import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLiveEvent, LIVE_PATH, useLiveUpdates } from "./useLiveUpdates";

/** A controllable stand-in for the browser's EventSource. */
class FakeEventSource extends EventTarget {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;
  static instances: FakeEventSource[] = [];
  readyState = FakeEventSource.OPEN;

  constructor(readonly url: string) {
    super();
    FakeEventSource.instances.push(this);
  }

  emit(name: string, data = "{}") {
    this.dispatchEvent(new MessageEvent(name, { data }));
  }

  fail() {
    this.readyState = FakeEventSource.CLOSED;
    this.dispatchEvent(new Event("error"));
  }

  close() {
    this.readyState = FakeEventSource.CLOSED;
  }
}

function seeded() {
  const queryClient = new QueryClient();
  for (const key of [
    ["tickets", "list", { view: "mine" }],
    ["tickets", "detail", "t1"],
    ["tickets", "timeline", "t1"],
    ["tickets", "detail", "t2"],
    ["lookups", "statuses"],
  ]) {
    queryClient.setQueryData(key, {});
  }
  return queryClient;
}

function stale(queryClient: QueryClient) {
  return queryClient
    .getQueryCache()
    .getAll()
    .filter((q) => q.state.isInvalidated)
    .map((q) => q.queryKey);
}

describe("Live Updates (FR-GEN-010)", () => {
  it("marks the list and the changed ticket stale, nothing else", () => {
    const queryClient = seeded();
    applyLiveEvent(queryClient, "invalidate", JSON.stringify({ topic: "ticket", id: "t1" }));
    expect(stale(queryClient)).toEqual([
      ["tickets", "list", { view: "mine" }],
      ["tickets", "detail", "t1"],
      ["tickets", "timeline", "t1"],
    ]);
  });

  it("refreshes who is on a ticket when presence changes", () => {
    const queryClient = seeded();
    queryClient.setQueryData(["tickets", "presence", "t1"], []);
    applyLiveEvent(
      queryClient,
      "invalidate",
      JSON.stringify({ topic: "ticket_presence", id: "t1" }),
    );
    expect(stale(queryClient)).toEqual([["tickets", "presence", "t1"]]);
  });

  it("refetches every ticket query on resync and ignores what it does not know", () => {
    const queryClient = seeded();
    applyLiveEvent(queryClient, "invalidate", "not json");
    applyLiveEvent(queryClient, "invalidate", JSON.stringify({ topic: "change", id: "c1" }));
    expect(stale(queryClient)).toEqual([]);
    applyLiveEvent(queryClient, "resync", "{}");
    expect(stale(queryClient)).toHaveLength(4);
  });

  describe("useLiveUpdates", () => {
    beforeEach(() => {
      vi.useFakeTimers();
      FakeEventSource.instances = [];
      vi.stubGlobal("EventSource", FakeEventSource);
    });
    afterEach(() => {
      vi.useRealTimers();
      vi.unstubAllGlobals();
    });

    function mount(queryClient: QueryClient, enabled = true) {
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      );
      return renderHook(({ on }) => useLiveUpdates(on), {
        wrapper,
        initialProps: { on: enabled },
      });
    }

    it("subscribes only while signed in and closes on unmount", () => {
      const queryClient = seeded();
      const hook = mount(queryClient, false);
      expect(FakeEventSource.instances).toHaveLength(0);
      hook.rerender({ on: true });
      expect(FakeEventSource.instances.map((s) => s.url)).toEqual([LIVE_PATH]);
      hook.unmount();
      expect(FakeEventSource.instances[0]?.readyState).toBe(FakeEventSource.CLOSED);
    });

    it("refetches after a reconnect but not on the first ready", () => {
      const queryClient = seeded();
      mount(queryClient);
      const source = FakeEventSource.instances[0];
      source?.emit("ready");
      expect(stale(queryClient)).toEqual([]);
      source?.emit("invalidate", JSON.stringify({ topic: "ticket", id: "t2" }));
      expect(stale(queryClient)).toContainEqual(["tickets", "detail", "t2"]);
      source?.emit("ready");
      expect(stale(queryClient)).toHaveLength(4);
    });

    it("reconnects with backoff when the browser gives up", () => {
      mount(seeded());
      FakeEventSource.instances[0]?.fail();
      expect(FakeEventSource.instances).toHaveLength(1);
      act(() => {
        vi.advanceTimersByTime(2_000);
      });
      expect(FakeEventSource.instances).toHaveLength(2);
      FakeEventSource.instances[1]?.fail();
      act(() => {
        vi.advanceTimersByTime(2_000);
      });
      expect(FakeEventSource.instances).toHaveLength(2);
      act(() => {
        vi.advanceTimersByTime(2_000);
      });
      expect(FakeEventSource.instances).toHaveLength(3);
    });
  });
});
