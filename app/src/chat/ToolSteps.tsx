import type { ToolStep } from "../ipc";
import { stepLabel } from "./chatReducer";

const Clock = () => (
  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
    <circle cx="8" cy="8" r="6" />
    <path d="M8 4.5V8l2.5 1.5" />
  </svg>
);

export function ToolSteps({ steps }: { steps: ToolStep[] }) {
  if (!steps.length) return null;
  return (
    <ul className="trace" aria-label="What bleep did">
      {steps.map((s) => (
        <li key={s.id}>
          <span className="ico"><Clock /></span>
          <span>{stepLabel(s.name)}</span>
          {s.ok === null ? <span className="spin" aria-label="running" /> : <span className={`res ${s.ok ? "" : "bad"}`}>{s.ok ? "done" : "failed"}</span>}
        </li>
      ))}
    </ul>
  );
}
