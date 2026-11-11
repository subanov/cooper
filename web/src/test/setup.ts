import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";

// Screens load through the router and the query cache; with many test files running in
// parallel that can take longer than Testing Library's 1 s default.
configure({ asyncUtilTimeout: 4000 });

// jsdom lacks layout APIs that cmdk and Radix call; real browsers have them.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;
Element.prototype.scrollIntoView ??= () => {};

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});
