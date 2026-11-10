/** Unwraps an openapi-fetch call for a query function: data, or an error to retry or show. */
export async function ok<T>(
  call: Promise<{ data?: T; response: Response }>,
  what: string,
): Promise<T> {
  const { data, response } = await call;
  if (data === undefined) {
    throw new Error(`${what} request failed: ${response.status}`);
  }
  return data;
}
