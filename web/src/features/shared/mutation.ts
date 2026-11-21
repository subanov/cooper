import { type QueryKey, useMutation, useQueryClient } from "@tanstack/react-query";
import { useToast } from "@/components/Toast";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import { problemOf } from "@/lib/problem";

/**
 * A mutation with consistent feedback (H1): the button shows a spinner while it runs,
 * success shows a toast, and the screen shows `error` next to the form (H9). Listed
 * queries are refetched afterwards.
 */
export function useAction<TVars, TData>(
  fn: (vars: TVars) => Promise<TData>,
  options: {
    invalidate?: readonly QueryKey[] | undefined;
    success?: MessageId | ((data: TData, vars: TVars) => string) | undefined;
    onSuccess?: ((data: TData, vars: TVars) => void | Promise<void>) | undefined;
    undo?:
      | ((data: TData, vars: TVars) => { label: string; run: () => void } | undefined)
      | undefined;
  } = {},
) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const t = useText();
  return useMutation({
    mutationFn: fn,
    onSuccess: async (data, vars) => {
      await Promise.all(
        (options.invalidate ?? []).map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      );
      if (options.success) {
        const message =
          typeof options.success === "function" ? options.success(data, vars) : t(options.success);
        const undo = options.undo?.(data, vars);
        toast({
          message,
          tone: "success",
          action: undo ? { label: undo.label, onClick: undo.run } : undefined,
        });
      }
      await options.onSuccess?.(data, vars);
    },
  });
}

/** The API's message for one field, when a validation problem names it. */
export function fieldError(error: unknown, field: string): string | undefined {
  if (!error) return undefined;
  const problem = problemOf(error);
  return problem.field === field ? (problem.detail ?? undefined) : undefined;
}

/** Whether a validation problem names a field (so the general notice can be skipped). */
export function isFieldProblem(error: unknown, fields: readonly string[]): boolean {
  if (!error) return false;
  const problem = problemOf(error);
  return problem.field !== undefined && fields.includes(problem.field);
}

/** Text from an optional input: empty becomes `null` (the API's "no value"). */
export function orNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}
