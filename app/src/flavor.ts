// Which build this is: prod ("bleep"), nightly ("bleep (nightly)") or local ("bleep (local)").
// Nightly and local builds set VITE_BLEEP_FLAVOR (see src-tauri/tauri.nightly.conf.json and
// tauri.local.conf.json). In headless UI checks, ?mock=1&flavor=local shows the local look.
const q = typeof window === "undefined" ? new URLSearchParams() : new URLSearchParams(window.location.search);
const fromQuery = q.has("mock") ? q.get("flavor") : null;
const raw = fromQuery ?? import.meta.env.VITE_BLEEP_FLAVOR ?? "";

export type Flavor = "prod" | "nightly" | "local";
export const FLAVOR: Flavor = raw === "nightly" || raw === "local" ? raw : "prod";
/** The short tag after the name, or "" for prod. */
export const TAG = FLAVOR === "prod" ? "" : FLAVOR;
/** A local build never updates itself. */
export const LOCAL = FLAVOR === "local";
export const NAME = TAG ? `bleep (${TAG})` : "bleep";
