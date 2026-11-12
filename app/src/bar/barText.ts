/** "in 1 hour", "in 42 min", "now". Rounds up, so a meeting 41.3 minutes away says 42. */
export function whenText(start: Date, now: Date): string {
  const mins = Math.ceil((start.getTime() - now.getTime()) / 60_000);
  if (mins <= 0) return "now";
  if (mins >= 55) return "in 1 hour";
  return `in ${mins} min`;
}

export function clockText(d: Date, timeZone?: string): string {
  return new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", timeZone }).format(d).replace(/ /g, " ");
}
