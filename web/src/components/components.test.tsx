import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { ApiError, toProblem } from "@/lib/problem";
import { axeViolations, renderUi } from "@/test/render";
import { Button, IconButton } from "./Button";
import { CheckboxList } from "./CheckboxList";
import { Combobox, type PickerOption } from "./Combobox";
import { DataTable } from "./DataTable";
import { Checkbox, SelectField, Switch, TextField } from "./Field";
import { Notice, ProblemNotice } from "./Notice";
import { Segmented } from "./Tabs";

const options: PickerOption[] = [
  { id: "1", label: "Ada Lovelace", description: "ada@example.com" },
  { id: "2", label: "Grace Hopper", description: "grace@example.com" },
];

function Harness() {
  const [value, setValue] = useState<PickerOption | null>(null);
  return (
    <>
      <Combobox label="Requester" value={value} onChange={setValue} options={options} />
      <output>{value?.label ?? "none"}</output>
    </>
  );
}

describe("design system", () => {
  it("labels fields and links hints and errors to them", async () => {
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
        <Checkbox label="Visible to the Requester" />
      </form>,
    );
    const input = screen.getByRole("textbox", { name: /Subject/ });
    expect(input).toBeRequired();
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription("Enter a subject. One line.");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows a busy button and blocks repeated clicks", async () => {
    const onClick = vi.fn();
    renderUi(
      <Button loading onClick={onClick}>
        Save
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Save" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
  });

  it("gives icon buttons their label as accessible name", () => {
    renderUi(<IconButton label="Close" icon={<span />} />);
    expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
  });

  it("explains problems in plain language, never with a status code", () => {
    const error = new ApiError(
      toProblem(
        { type: "x/forbidden", detail: "Needs tickets.update." },
        new Response(null, { status: 403 }),
      ),
    );
    renderUi(<ProblemNotice error={error} />);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("You do not have permission to do this");
    expect(alert).toHaveTextContent("Needs tickets.update.");
    expect(alert).not.toHaveTextContent("403");
  });

  it("announces notices politely unless they report danger", () => {
    renderUi(<Notice tone="success">Saved.</Notice>);
    expect(screen.getByRole("status")).toHaveTextContent("Saved.");
  });

  it("selects an option of a combobox with the keyboard", async () => {
    const user = userEvent.setup();
    const { container } = renderUi(<Harness />);
    const box = screen.getByRole("combobox", { name: "Requester" });
    await user.type(box, "grace");
    expect(box).toHaveAttribute("aria-expanded", "true");
    const list = screen.getByRole("listbox");
    // Typing is debounced before the list narrows.
    await waitFor(() => expect(within(list).getAllByRole("option")).toHaveLength(1));
    await user.keyboard("{Enter}");
    expect(screen.getByText("Grace Hopper", { selector: "output" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("filters long checkbox lists and counts the chosen items", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const items = Array.from({ length: 10 }, (_, i) => ({ id: `c${i}`, label: `tickets.cap${i}` }));
    renderUi(
      <CheckboxList legend="Capabilities" items={items} selected={["c1"]} onChange={onChange} />,
    );
    expect(screen.getByText("1 of 10 chosen")).toBeInTheDocument();
    await user.type(screen.getByRole("searchbox", { name: "Filter Capabilities" }), "cap3");
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
    await user.click(screen.getByRole("checkbox", { name: "tickets.cap3" }));
    expect(onChange).toHaveBeenCalledWith(["c1", "c3"]);
  });

  it("switches report their state", () => {
    const onChange = vi.fn();
    renderUi(<Switch label="Tasks Module" checked={false} onChange={onChange} />);
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
    expect(screen.getByRole("group", { name: "View" })).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: "List" }));
    expect(onChange).toHaveBeenCalledWith("list");
  });

  it("renders tables with a caption, headers and an empty state", async () => {
    const { container } = renderUi(
      <DataTable
        caption="Assets"
        rows={[]}
        rowKey={(r: { id: string }) => r.id}
        columns={[{ id: "n", header: "Name", cell: (r) => r.id }]}
        empty={<p>No Assets.</p>}
      />,
    );
    expect(screen.getByRole("table", { name: "Assets" })).toBeInTheDocument();
    expect(screen.getByText("No Assets.")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });
});
