import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { ApiError, toProblem } from "@/lib/problem";
import { axeViolations, renderUi } from "@/test/render";
import { Badge } from "./Badge";
import { Button, IconButton } from "./Button";
import { CheckboxList } from "./CheckboxList";
import { Combobox, type PickerOption } from "./Combobox";
import { DataTable } from "./DataTable";
import { ConfirmDialog, FormDialog } from "./Dialog";
import { EmptyState, Notice, ProblemNotice } from "./Feedback";
import { Checkbox, SelectField, Switch, TextField } from "./Field";
import { PropertyList, PropertySelect } from "./PropertyList";
import { Segmented, Tabs } from "./Tabs";
import { useToast } from "./Toast";

/** The live region that holds `text` (toasts keep their own empty regions mounted). */
function region(role: "alert" | "status", text: string): HTMLElement {
  const found = screen.getAllByRole(role).find((el) => el.textContent?.includes(text));
  if (!found) throw new Error(`no ${role} with "${text}"`);
  return found;
}

const people: PickerOption[] = [
  { id: "1", label: "Ada Lovelace", description: "ada@example.com" },
  { id: "2", label: "Grace Hopper", description: "grace@example.com" },
];

function PickerHarness() {
  const [value, setValue] = useState<PickerOption | null>(null);
  return (
    <>
      <Combobox label="Requester" value={value} onChange={setValue} options={people} />
      <output>{value?.label ?? "none"}</output>
    </>
  );
}

function ToastHarness({ onUndo }: { onUndo: () => void }) {
  const toast = useToast();
  return (
    <Button
      onClick={() =>
        toast({
          tone: "success",
          message: "Assigned to you.",
          action: { label: "Undo", onClick: onUndo },
        })
      }
    >
      Assign
    </Button>
  );
}

describe("design system", () => {
  it("labels fields and ties hints and errors to them", async () => {
    const { container } = renderUi(
      <form>
        <TextField
          label="Subject"
          hint="One line."
          error="Enter a subject."
          required
          value=""
          onChange={() => {}}
        />
        <SelectField
          label="Priority"
          options={[{ value: "p", label: "Normal" }]}
          value="p"
          onChange={() => {}}
        />
        <Checkbox label="Visible to the Requester" hint="Shown on the Portal." />
      </form>,
    );
    const input = screen.getByRole("textbox", { name: /Subject/ });
    expect(input).toBeRequired();
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription("Enter a subject. One line.");
    expect(
      screen.getByRole("checkbox", { name: "Visible to the Requester" }),
    ).toHaveAccessibleDescription("Shown on the Portal.");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows a busy button and blocks repeated clicks", () => {
    renderUi(<Button loading>Save</Button>);
    const button = screen.getByRole("button", { name: "Save" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
  });

  it("gives icon buttons their label as name and tooltip", () => {
    renderUi(<IconButton label="Copy Ticket Number" icon={<span />} />);
    const button = screen.getByRole("button", { name: "Copy Ticket Number" });
    expect(button).toHaveAttribute("data-tip", "Copy Ticket Number");
  });

  it("explains problems in plain language, never with a status code", () => {
    const error = new ApiError(
      toProblem(
        { type: "x/forbidden", detail: "Needs tickets.update." },
        new Response(null, { status: 403 }),
      ),
    );
    renderUi(<ProblemNotice error={error} />);
    const alert = region("alert", "permission");
    expect(alert).toHaveTextContent("You do not have permission to do this");
    expect(alert).toHaveTextContent("Needs tickets.update.");
    expect(alert).not.toHaveTextContent("403");
  });

  it("tells what to do next when the server fails, and offers Try again", async () => {
    const user = userEvent.setup();
    const retry = vi.fn();
    const error = new ApiError(
      toProblem({ detail: "SQL said no" }, new Response(null, { status: 500 })),
    );
    renderUi(<ProblemNotice error={error} onRetry={retry} />);
    const alert = region("alert", "server");
    expect(alert).toHaveTextContent("Something went wrong on the server");
    expect(alert).not.toHaveTextContent("SQL");
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    expect(retry).toHaveBeenCalled();
  });

  it("announces notices politely unless they report danger", () => {
    renderUi(<Notice tone="success">Saved.</Notice>);
    expect(region("status", "Saved.")).toBeInTheDocument();
  });

  it("selects an option of a combobox with the keyboard", async () => {
    const user = userEvent.setup();
    const { container } = renderUi(<PickerHarness />);
    const box = screen.getByRole("combobox", { name: "Requester" });
    await user.type(box, "grace");
    expect(box).toHaveAttribute("aria-expanded", "true");
    const list = screen.getByRole("listbox");
    await waitFor(() => expect(within(list).getAllByRole("option")).toHaveLength(1));
    await user.keyboard("{Enter}");
    expect(screen.getByText("Grace Hopper", { selector: "output" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Clear Requester" }));
    expect(screen.getByText("none", { selector: "output" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("filters long checkbox lists and counts the chosen items", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const options = Array.from({ length: 10 }, (_, i) => ({
      value: `c${i}`,
      label: `tickets.cap${i}`,
    }));
    renderUi(
      <CheckboxList legend="Capabilities" options={options} value={["c1"]} onChange={onChange} />,
    );
    expect(screen.getByText("1 of 10 chosen")).toBeInTheDocument();
    await user.type(screen.getByRole("searchbox", { name: "Filter Capabilities" }), "cap3");
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
    await user.click(screen.getByRole("checkbox", { name: "tickets.cap3" }));
    expect(onChange).toHaveBeenCalledWith(["c1", "c3"]);
  });

  it("switches report their state", () => {
    const onChange = vi.fn();
    renderUi(<Switch label="Tasks Module" checked={false} onCheckedChange={onChange} />);
    const sw = screen.getByRole("switch", { name: "Tasks Module" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("segmented controls are radio groups", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderUi(
      <Segmented
        label="View"
        value="month"
        onChange={onChange}
        options={[
          { value: "month", label: "Month" },
          { value: "list", label: "List" },
        ]}
      />,
    );
    expect(screen.getByRole("radiogroup", { name: "View" })).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: "List" }));
    expect(onChange).toHaveBeenCalledWith("list");
  });

  it("moves between tabs with the arrow keys", async () => {
    const user = userEvent.setup();
    const { container } = renderUi(
      <Tabs
        label="Parts"
        tabs={[
          { id: "a", label: "Details", content: <p>First</p> },
          { id: "b", label: "Members", content: <p>Second</p> },
        ]}
      />,
    );
    screen.getByRole("tab", { name: "Details" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Members" })).toHaveFocus();
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Second");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("renders tables with a caption and headers, or the empty state", async () => {
    const rows = [{ id: "LT-1" }];
    const { container, rerender } = renderUi(
      <DataTable
        caption="Assets"
        rows={rows}
        rowKey={(r) => r.id}
        columns={[{ key: "n", header: "Name", cell: (r) => r.id }]}
      />,
    );
    expect(screen.getByRole("table", { name: "Assets" })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "Name" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    rerender(
      <DataTable
        caption="Assets"
        rows={[] as { id: string }[]}
        rowKey={(r) => r.id}
        columns={[{ key: "n", header: "Name", cell: (r) => r.id }]}
        empty={<EmptyState title="No Assets." />}
      />,
    );
    expect(screen.getByText("No Assets.")).toBeInTheDocument();
  });

  it("changes a property in place and shows that it is saving", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { container, rerender } = renderUi(
      <PropertyList>
        <PropertySelect
          label="Priority"
          value="n"
          onChange={onChange}
          options={[
            { value: "n", label: "Normal" },
            { value: "h", label: "High" },
          ]}
        />
      </PropertyList>,
    );
    await user.selectOptions(screen.getByRole("combobox", { name: "Priority" }), "h");
    expect(onChange).toHaveBeenCalledWith("h");
    rerender(
      <PropertyList>
        <PropertySelect
          saving
          label="Priority"
          value="n"
          onChange={onChange}
          options={[{ value: "n", label: "Normal" }]}
        />
      </PropertyList>,
    );
    expect(screen.getByRole("combobox", { name: "Priority" })).toHaveAttribute("aria-busy", "true");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("offers Undo in a toast instead of asking first", async () => {
    const user = userEvent.setup();
    const onUndo = vi.fn();
    renderUi(<ToastHarness onUndo={onUndo} />);
    await user.click(screen.getByRole("button", { name: "Assign" }));
    expect(screen.getByText("Assigned to you.").closest("[aria-live=polite]")).not.toBeNull();
    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(onUndo).toHaveBeenCalled();
  });

  it("form dialogs submit with Enter and close with Cancel", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const onOpenChange = vi.fn();
    renderUi(
      <FormDialog
        open
        onOpenChange={onOpenChange}
        title="Snooze ACM-1"
        submitLabel="Snooze"
        onSubmit={onSubmit}
      >
        <TextField label="Reason" />
      </FormDialog>,
    );
    const dialog = screen.getByRole("dialog", { name: "Snooze ACM-1" });
    await user.type(within(dialog).getByLabelText("Reason"), "Waiting{Enter}");
    expect(onSubmit).toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("confirms destructive actions naming the object", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    renderUi(
      <ConfirmDialog
        open
        onOpenChange={() => {}}
        title="Delete ACM-42?"
        confirmLabel="Delete ticket"
        onConfirm={onConfirm}
      >
        <p>It leaves every view.</p>
      </ConfirmDialog>,
    );
    await user.click(screen.getByRole("button", { name: "Delete ticket" }));
    expect(onConfirm).toHaveBeenCalled();
  });

  it("badges carry their meaning in text", () => {
    renderUi(<Badge tone="danger">Breached</Badge>);
    expect(screen.getByText("Breached")).toBeInTheDocument();
  });
});
