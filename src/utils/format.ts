/** Small formatting helpers shared by the UI. */

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}

export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatRelative(iso: string | null | undefined): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const diffMs = date.getTime() - Date.now();
  const abs = Math.abs(diffMs);
  const minutes = Math.round(abs / 60_000);
  const hours = Math.round(abs / 3_600_000);
  const days = Math.round(abs / 86_400_000);
  const suffix = diffMs < 0 ? "ago" : "from now";
  if (minutes < 1) return diffMs < 0 ? "just now" : "now";
  if (minutes < 60) return `${minutes} min ${suffix}`;
  if (hours < 48) return `${hours} h ${suffix}`;
  return `${days} d ${suffix}`;
}

/** Build the local ISO 8601 string (minute precision) the reminder engine expects. */
export function toLocalIsoMinute(date: Date): string {
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}`;
}

/** Replace `[x]` with `[ ]` so two markdown strings can be compared ignoring checkbox state. */
export function withoutCheckboxState(markdown: string): string {
  return markdown.replace(/^(\s*(?:[-*+]|\d+[.)])\s+)\[[ xX]\]/gm, "$1[ ]");
}

export function fileStem(name: string): string {
  const idx = name.lastIndexOf(".");
  return idx > 0 ? name.slice(0, idx) : name;
}
