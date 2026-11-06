import { useIntl } from "react-intl";
import type { MessageId } from "./messages";

/** Formats a catalogue message by id, with type-checked ids. */
export function useText(): (id: MessageId, values?: Record<string, string | number>) => string {
  const intl = useIntl();
  return (id, values) => intl.formatMessage({ id }, values);
}
