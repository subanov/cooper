/** Helpers that read list state from the URL (filters, sort, selection; H3). */

type Raw = Record<string, unknown>;

/** A non-empty string, or nothing. */
export function text(search: Raw, key: string): string | undefined {
  const v = search[key];
  return typeof v === "string" && v.trim() !== ""
    ? v
    : typeof v === "number"
      ? String(v)
      : undefined;
}

/** One of the allowed values, or nothing. */
export function oneOf<T extends string>(
  search: Raw,
  key: string,
  values: readonly T[],
): T | undefined {
  const v = search[key];
  return typeof v === "string" && (values as readonly string[]).includes(v) ? (v as T) : undefined;
}

/** `true` when the flag is set (`?closed=true` or `?closed`). */
export function flag(search: Raw, key: string): true | undefined {
  const v = search[key];
  return v === true || v === "true" || v === "" || v === 1 ? true : undefined;
}

/** Drops keys whose value is undefined (exactOptionalPropertyTypes). */
export function compact<T extends Record<string, unknown>>(
  value: T,
): { [K in keyof T]?: Exclude<T[K], undefined> } {
  return Object.fromEntries(Object.entries(value).filter(([, v]) => v !== undefined)) as {
    [K in keyof T]?: Exclude<T[K], undefined>;
  };
}

/** A change to list search state: `undefined` removes a key. */
export type SearchPatch<T> = { [K in keyof T]?: T[K] | undefined };
