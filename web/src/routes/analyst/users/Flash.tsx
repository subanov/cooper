import { useState } from "react";

/** A one-line result message next to a form: announced politely, errors as alerts. */
export function useFlash() {
  const [flash, setFlash] = useState<{ text: string; error: boolean } | null>(null);
  const element = flash ? (
    <p role={flash.error ? "alert" : "status"} className="text-sm">
      {flash.text}
    </p>
  ) : null;
  return {
    ok: (text: string) => setFlash({ text, error: false }),
    fail: (text: string) => setFlash({ text, error: true }),
    clear: () => setFlash(null),
    element,
  };
}
