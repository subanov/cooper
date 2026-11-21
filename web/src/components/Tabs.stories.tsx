import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { Segmented } from "./Tabs";

function Views() {
  const [value, setValue] = useState<"oldest" | "newest">("oldest");
  return (
    <Segmented
      label="Order"
      value={value}
      onChange={setValue}
      options={[
        { value: "oldest", label: "Oldest first" },
        { value: "newest", label: "Newest first" },
      ]}
    />
  );
}

const meta = { title: "Design system/Segmented", component: Views } satisfies Meta<typeof Views>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
