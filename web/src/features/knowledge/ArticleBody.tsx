import { cn } from "@/lib/cn";

/**
 * Article HTML. The API sanitises every body with ammonia before storing it (AGENTS.md
 * §10), and the Content-Security-Policy blocks inline scripts as a second layer.
 */
export function ArticleBody({ html, className }: { html: string; className?: string | undefined }) {
  return (
    <div
      className={cn(
        "max-w-prose text-[0.9375rem] leading-relaxed [&_a]:text-accent-text [&_a]:underline [&_code]:rounded-sm [&_code]:bg-sunken [&_code]:px-1 [&_h1]:mb-2 [&_h1]:mt-5 [&_h1]:text-xl [&_h1]:font-semibold [&_h2]:mb-2 [&_h2]:mt-5 [&_h2]:text-lg [&_h2]:font-semibold [&_h3]:mb-1 [&_h3]:mt-4 [&_h3]:font-semibold [&_li]:my-1 [&_ol]:list-decimal [&_ol]:pl-6 [&_p]:my-3 [&_pre]:overflow-x-auto [&_pre]:rounded-md [&_pre]:bg-sunken [&_pre]:p-3 [&_table]:my-3 [&_table]:border-collapse [&_td]:border [&_td]:border-line [&_td]:p-2 [&_th]:border [&_th]:border-line [&_th]:p-2 [&_ul]:list-disc [&_ul]:pl-6",
        className,
      )}
      // biome-ignore lint/security/noDangerouslySetInnerHtml: the API returns sanitised HTML only
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
