import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axeViolations } from "@/test/render";
import { Button } from "./button";

describe("Button", () => {
  it("is keyboard operable and accessible", async () => {
    const onClick = vi.fn();
    const { container } = render(<Button onClick={onClick}>Assign to me</Button>);
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "Assign to me" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    expect(onClick).toHaveBeenCalledOnce();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("styles a link as a button with asChild", () => {
    render(
      <Button asChild variant="link">
        <a href="/portal">Portal</a>
      </Button>,
    );
    expect(screen.getByRole("link", { name: "Portal" })).toHaveClass("underline-offset-4");
  });
});
