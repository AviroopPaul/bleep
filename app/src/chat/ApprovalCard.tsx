import { useRef, useState } from "react";
import { api, type Approval } from "../ipc";
import { approvalView } from "./approvalView";

/** The exact change a write tool wants to make. Nothing runs until Approve. */
export function ApprovalCard({ a }: { a: Approval }) {
  const v = approvalView(a);
  const sent = useRef(false);
  const [busy, setBusy] = useState(false);
  const pending = a.status === "pending";
  const decide = (approve: boolean) => {
    // A ref, not state: two fast clicks land before React re-renders.
    if (sent.current) return;
    sent.current = true;
    setBusy(true);
    api.decide(a.id, approve).catch(() => {});
  };
  return (
    <section className={`card ${a.status}`} aria-label={v.heading}>
      <h2 className="card-h">{v.heading}</h2>
      <dl>
        {v.rows.map(([k, val]) => (
          <div key={k}>
            <dt>{k}</dt>
            <dd>{val}</dd>
          </div>
        ))}
      </dl>
      {v.warn && <p className="card-warn">{v.warn}</p>}
      {pending ? (
        <div className="acts">
          <button className="btn pri" disabled={busy} onClick={() => decide(true)}>Approve</button>
          <button className="btn" disabled={busy} onClick={() => decide(false)}>Decline</button>
        </div>
      ) : (
        <p className={`card-done ${a.status}`} role="status">{v.doneText}</p>
      )}
    </section>
  );
}
