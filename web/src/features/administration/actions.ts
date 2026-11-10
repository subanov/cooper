import type { Result } from "@/features/session/actions";
import { api, type Schemas } from "@/lib/api";
import { toProblem } from "@/lib/problem";

async function result<T>(
  call: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<Result<T>> {
  const { data, error, response } = await call;
  return data !== undefined
    ? { ok: true, data }
    : { ok: false, problem: toProblem(error, response) };
}

/** Changes a System Setting. The server validates the value against its kind. */
export function updateSetting(key: string, value: unknown): Promise<Result<Schemas["SettingDto"]>> {
  return result(
    api.PUT("/api/v1/settings/{key}", {
      params: { path: { key } },
      body: { value: value as Schemas["SettingRequest"]["value"] },
    }),
  );
}

/** Turns a Module on or off. */
export function setModule(key: string, is_enabled: boolean): Promise<Result<Schemas["ModuleDto"]>> {
  return result(
    api.PUT("/api/v1/modules/{key}", { params: { path: { key } }, body: { is_enabled } }),
  );
}

/** Sets the schedule of a Background Job. */
export function setJobSchedule(
  name: string,
  body: Schemas["ScheduleDto"],
): Promise<Result<Schemas["JobDto"]>> {
  return result(
    api.PUT("/api/v1/background-jobs/{name}/schedule", { params: { path: { name } }, body }),
  );
}

/** Asks a worker to run a Background Job now. */
export function runJob(name: string): Promise<Result<Schemas["JobDto"]>> {
  return result(api.POST("/api/v1/background-jobs/{name}/run", { params: { path: { name } } }));
}
