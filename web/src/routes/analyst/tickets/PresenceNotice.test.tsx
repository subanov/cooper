import { render, screen } from "@testing-library/react";
import { IntlProvider } from "react-intl";
import { describe, expect, it } from "vitest";
import { en } from "@/i18n/messages";
import { PresenceNotice } from "./PresenceNotice";

function renderNotice(viewers: Parameters<typeof PresenceNotice>[0]["viewers"]) {
  return render(
    <IntlProvider locale="en" messages={en}>
      <PresenceNotice viewers={viewers} names={new Map([["a1", "Ann Analyst"]])} />
    </IntlProvider>,
  );
}

describe("Ticket Presence (FR-TKT-025)", () => {
  it("names who is viewing and who is writing", () => {
    renderNotice([
      { analyst_id: "a1", is_composing: true, last_seen: "2026-11-07T10:00:00Z" },
      { analyst_id: "a2", is_composing: false, last_seen: "2026-11-07T10:00:00Z" },
    ]);
    expect(screen.getByText("Ann Analyst is writing a reply or Note")).toBeInTheDocument();
    expect(screen.getByText("Another Analyst is viewing this ticket")).toBeInTheDocument();
  });

  it("shows nothing when nobody else is there", () => {
    const { container } = renderNotice([]);
    expect(container).toBeEmptyDOMElement();
  });
});
