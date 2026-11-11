import { queryOptions } from "@tanstack/react-query";
import { ok } from "@/features/okQuery";
import { result } from "@/features/result";
import { api, type Schemas } from "@/lib/api";

/** Calendar Events touching `from`..=`to` (dates), optionally in one Calendar Category. */
export function calendarEventsQuery(filter: { from: string; to: string; category?: string }) {
  return queryOptions({
    queryKey: ["calendar", "events", filter],
    queryFn: () =>
      ok(
        api.GET("/api/v1/calendar/events", {
          params: {
            query: {
              from: filter.from,
              to: filter.to,
              ...(filter.category ? { category_id: filter.category } : {}),
            },
          },
        }),
        "calendar events",
      ),
  });
}

/** Every Calendar Category. */
export const calendarCategoriesQuery = queryOptions({
  queryKey: ["lookups", "calendar-categories"],
  queryFn: () => ok(api.GET("/api/v1/calendar/categories"), "calendar categories"),
  staleTime: 5 * 60_000,
});

/** Adds a manual Calendar Event. */
export const createCalendarEvent = (body: Schemas["CalendarEventRequest"]) =>
  result(api.POST("/api/v1/calendar/events", { body }));

/** Changes a manual Calendar Event. */
export const updateCalendarEvent = (id: string, body: Schemas["CalendarEventRequest"]) =>
  result(api.PUT("/api/v1/calendar/events/{id}", { params: { path: { id } }, body }));

/** Deletes a manual Calendar Event. */
export const deleteCalendarEvent = (id: string) =>
  result(api.DELETE("/api/v1/calendar/events/{id}", { params: { path: { id } } }));

/** Adds a Calendar Category. */
export const createCalendarCategory = (body: Schemas["CalendarCategoryRequest"]) =>
  result(api.POST("/api/v1/calendar/categories", { body }));

/** Changes a Calendar Category. */
export const updateCalendarCategory = (id: string, body: Schemas["CalendarCategoryRequest"]) =>
  result(api.PUT("/api/v1/calendar/categories/{id}", { params: { path: { id } }, body }));
