import { queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";

/** Every Module and whether it is on (FR-GEN-005). */
export const modulesQuery = queryOptions({
  queryKey: ["modules"],
  queryFn: async (): Promise<Schemas["ModuleDto"][]> => {
    const { data, response } = await api.GET("/api/v1/modules");
    if (!data) {
      throw new Error(`modules request failed: ${response.status}`);
    }
    return data;
  },
  staleTime: 5 * 60_000,
});
