/** Cooper's mark: decorative, the name is always written next to it. */
export function Logo({ className = "size-8" }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" aria-hidden="true" className={className}>
      <rect width="32" height="32" rx="8" className="fill-accent" />
      <path
        d="M21.5 11.2A7 7 0 1 0 21.5 20.8"
        fill="none"
        strokeWidth="3.2"
        strokeLinecap="round"
        className="stroke-accent-fg"
      />
      <circle cx="22.4" cy="16" r="1.9" className="fill-accent-fg" />
    </svg>
  );
}
