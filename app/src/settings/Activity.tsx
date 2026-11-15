import { useEffect, useState } from "react";
import { api, type ActivityItem, type SpendView } from "../ipc";
import { activityTime, activityTitle } from "./activityText";
import { spendText } from "./modelText";

const ICON: Record<string, string> = { tool: "T", approval: "A", model: "M", agent: "C" };

/** Every tool call, approval card and model call, newest first. */
export function Activity() {
  const [items, setItems] = useState<ActivityItem[] | null>(null);
  const [spend, setSpend] = useState<SpendView | null>(null);
  useEffect(() => {
    api.activityList(200).then(setItems).catch(() => setItems([]));
    api.spendToday().then(setSpend).catch(() => {});
  }, []);
  if (!items) return null;
  const money = spend && spend.runtime === "api" ? <p className={`set-lead ${spend.over ? "set-warn" : ""}`} role="status" aria-label="Spend">{spendText(spend)}</p> : null;
  if (!items.length) return <>{money}<p className="set-lead">Nothing yet.</p></>;
  return (
    <>
    {money}
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
    </>
  );
}
