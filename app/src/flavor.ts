// Which build this is. "bleep (dev)" is built with VITE_BLEEP_DEV=1 (see npm run build:dev).
// In headless UI checks, ?mock=1&dev=1 shows the dev look.
const q = typeof window === "undefined" ? new URLSearchParams() : new URLSearchParams(window.location.search);

export const DEV = import.meta.env.VITE_BLEEP_DEV === "1" || (q.has("mock") && q.has("dev"));
export const NAME = DEV ? "bleep (dev)" : "bleep";
