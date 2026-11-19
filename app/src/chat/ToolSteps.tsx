import { useState } from "react";
import { ChevronDown, CircleAlert, CircleCheck, LoaderCircle } from "lucide-react";
import type { ToolStep } from "../ipc";
import { stepLabel } from "./chatReducer";
import { stepsSummary } from "./chatText";

/** One quiet line for what bleep did this turn. Open it for each step. */
export function ToolSteps({ steps }: { steps: ToolStep[] }) {
  const [open, setOpen] = useState(false);
  if (!steps.length) return null;
  const s = stepsSummary(steps);
  return (
    <div className="steps-box">
      <button type="button" className="steps-sum" aria-expanded={open} onClick={() => setOpen(!open)}>
        {s.running ? <LoaderCircle className="spin-ico" aria-hidden="true" /> : s.failed ? <CircleAlert className="bad" aria-hidden="true" /> : <CircleCheck className="ok" aria-hidden="true" />}
        <span className={s.running ? "shimmer" : undefined}>{s.running ?? s.text}</span>
        {!s.running && s.failed > 0 && <span className="steps-bad">{s.failed} failed</span>}
        <ChevronDown className="chev" aria-hidden="true" />
      </button>
      <ul className="trace" aria-label="What bleep did" hidden={!open}>
        {steps.map((st) => (
          <li key={st.id}>
            {st.ok === null ? <LoaderCircle className="spin-ico" aria-label="running" /> : st.ok ? <CircleCheck className="ok" aria-label="done" /> : <CircleAlert className="bad" aria-label="failed" />}
            <span>{stepLabel(st.name)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
