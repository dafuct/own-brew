import type { ReactNode } from "react";

export function Tag({
  variant,
  children,
  title,
}: {
  variant: "installed" | "outdated" | "deprecated" | "disabled" | "pinned" | "rollback";
  children: ReactNode;
  title?: string;
}) {
  return (
    <span className={`tag tag--${variant}`} title={title}>
      {children}
    </span>
  );
}

/** Popularity shown as a bar: comparable at a glance, unlike a raw count. */
export function PopularityBar({ installs }: { installs: number | null }) {
  if (installs === null) return null;
  // Install counts span six orders of magnitude, so scale logarithmically.
  const fraction = Math.min(1, Math.log10(Math.max(installs, 1)) / 6.2);
  return (
    <div
      className="popbar"
      title={`${installs.toLocaleString()} installs in the last 90 days`}
      aria-label={`${installs.toLocaleString()} installs in the last 90 days`}
    >
      <div className="popbar__fill" style={{ width: `${Math.round(fraction * 100)}%` }} />
    </div>
  );
}

export function relativeTime(unixSeconds: number | null): string {
  if (!unixSeconds) return "—";
  const days = Math.floor((Date.now() / 1000 - unixSeconds) / 86_400);
  if (days < 1) return "today";
  if (days === 1) return "yesterday";
  if (days < 30) return `${days} days ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months} month${months === 1 ? "" : "s"} ago`;
  const years = Math.floor(days / 365);
  return `${years} year${years === 1 ? "" : "s"} ago`;
}
