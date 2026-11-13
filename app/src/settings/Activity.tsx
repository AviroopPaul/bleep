import { useEffect, useState } from "react";
import { api, type ActivityItem } from "../ipc";
import { activityTime, activityTitle } from "./activityText";

const ICON: Record<string, string> = { tool: "T", approval: "A", model: "M", agent: "C" };

/** Every tool call, approval card and model call, newest first. */
export function Activity() {
  const [items, setItems] = useState<ActivityItem[] | null>(null);
  useEffect(() => {
    api.activityList(200).then(setItems).catch(() => setItems([]));
  }, []);
  if (!items) return null;
  if (!items.length) return <p className="set-lead">Nothing yet.</p>;
  return (
    <ul className="act" aria-label="Activity">
      {items.map((a, i) => (
        <li key={i} className={a.ok ? "" : "bad"}>
          <span className={`act-ico ${a.kind}`} aria-hidden="true">{ICON[a.kind] ?? "?"}</span>
          <span className="act-title">{activityTitle(a)}</span>
          {!a.ok && <span className="act-bad">failed</span>}
          <span className="act-time">{activityTime(a.at)}</span>
        </li>
      ))}
    </ul>
  );
}
