import { useCallback, useEffect, useState } from "react";
import { api, type ChatContext, type UsageView as Usage } from "../ipc";
import { meterLabel, meterLevel } from "../settings/memoryText";
import { busiest, costNote, count, dayLabel, heatLevel, hourLabel, money, purposeLabel, RANGES, runtimeLabel, SERIES, short, stacks, WEEKDAYS } from "./usageText";

const BAR_H = 96;

/** Settings > Usage: what bleep spent and when, from the local database. */
export function UsageView({ ctx }: { ctx: ChatContext | null }) {
  const [days, setDays] = useState(7);
  const [v, setV] = useState<Usage | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    api.usageSummary(days).then((u) => (setV(u), setErr(null))).catch((e) => setErr(String(e)));
  }, [days]);
  useEffect(load, [load]);

  const used = ctx?.used ?? null;
  const ctxPct = ctx && used != null ? Math.min(100, (used / ctx.threshold) * 100) : 0;

  return (
    <div className="usage">
      <div className="us-range" role="radiogroup" aria-label="Range">
        {RANGES.map((r) => (
          <button key={r.days} role="radio" aria-checked={days === r.days} className="us-pill" onClick={() => setDays(r.days)}>{r.label}</button>
        ))}
      </div>

      {err && <p className="us-empty" role="alert">{err}</p>}

      <div className="us-tiles">
        <div className="us-tile">
          <span className="us-k">Model calls</span>
          <b className="us-v">{v ? count(v.calls) : "-"}</b>
          <span className="us-n">{v && v.failed ? `${v.failed} failed` : "None failed"}</span>
        </div>
        <div className="us-tile">
          <span className="us-k">Tokens</span>
          <b className="us-v">{v ? short(v.tokens_in + v.tokens_out) : "-"}</b>
          <span className="us-n">{v ? `${short(v.tokens_in)} in, ${short(v.tokens_out)} out` : ""}</span>
        </div>
        <div className="us-tile">
          <span className="us-k">Cost</span>
          <b className="us-v">{v?.cost != null ? money(v.cost) : "$0"}</b>
          <span className="us-n">{v ? costNote(v) : ""}</span>
        </div>
        <div className={`us-tile ${ctx && used != null && meterLevel(used, ctx.threshold) === "high" ? "high" : ""}`}>
          <span className="us-k">Chat context</span>
          <b className="us-v">{ctx && used != null ? meterLabel(used, ctx.threshold) : "-"}</b>
          <span
            className="us-ctx"
            role="meter"
            aria-label="Chat context"
            aria-valuemin={0}
            aria-valuemax={ctx?.threshold ?? 0}
            aria-valuenow={used ?? 0}
            aria-valuetext={ctx ? meterLabel(used, ctx.threshold) : ""}
            title="bleep compacts the chat at the second number. Your history stays."
          >
            <i style={{ width: `${ctxPct}%` }} />
          </span>
        </div>
      </div>

      {v && v.calls === 0 && <p className="us-empty">No model calls in this range yet.</p>}

      {v && v.calls > 0 && (
        <>
          <section className="us-sec" aria-labelledby="us-heat-h">
            <div className="us-sec-h">
              <h2 id="us-heat-h">When bleep works</h2>
              <span>{busiest(v.heat)}</span>
            </div>
            <Heatmap heat={v.heat} />
          </section>

          <section className="us-sec" aria-labelledby="us-tok-h">
            <div className="us-sec-h">
              <h2 id="us-tok-h">Tokens per day</h2>
            </div>
            <Bars v={v} />
          </section>

          <section className="us-sec" aria-labelledby="us-mod-h">
            <div className="us-sec-h"><h2 id="us-mod-h">By model</h2></div>
            <table className="us-table">
              <thead>
                <tr><th scope="col">Model</th><th scope="col">Calls</th><th scope="col">Tokens</th><th scope="col">Cost</th></tr>
              </thead>
              <tbody>
                {v.models.map((m) => (
                  <tr key={`${m.runtime}/${m.model}`}>
                    <td><b>{m.model}</b><span>{runtimeLabel(m.runtime)}</span></td>
                    <td>{count(m.calls)}</td>
                    <td>{short(m.tokens_in + m.tokens_out)}</td>
                    <td>{m.cost != null ? money(m.cost) : "-"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        </>
      )}

      {v && v.tools.length > 0 && (
        <section className="us-sec" aria-labelledby="us-tools-h">
          <div className="us-sec-h"><h2 id="us-tools-h">Tools</h2></div>
          <ul className="us-tools">
            {v.tools.map((t) => (
              <li key={t.tool}>
                <span className="us-tool">{t.tool}</span>
                <span className="us-track"><i style={{ width: `${(t.calls / v.tools[0].calls) * 100}%` }} /></span>
                <span className="us-num">{count(t.calls)}{t.failed > 0 && <em> ({t.failed} failed)</em>}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {v && Object.values(v.agents).some((n) => n > 0) && (
        <section className="us-sec" aria-labelledby="us-ag-h">
          <div className="us-sec-h"><h2 id="us-ag-h">Coding agents</h2></div>
          <div className="us-agents">
            <span><b>{v.agents.auto}</b> auto-approved</span>
            <span><b>{v.agents.approved}</b> approved by you</span>
            <span><b>{v.agents.denied}</b> denied</span>
            <span><b>{v.agents.answered}</b> answered</span>
            {v.agents.expired > 0 && <span><b>{v.agents.expired}</b> timed out</span>}
          </div>
        </section>
      )}
    </div>
  );
}

function Heatmap({ heat }: { heat: number[][] }) {
  const max = Math.max(0, ...heat.flat());
  return (
    <div className="us-heat" role="img" aria-label="Model calls by weekday and hour">
      {heat.map((row, d) => (
        <div className="us-hrow" key={d}>
          <span className="us-wd">{WEEKDAYS[d]}</span>
          {row.map((n, h) => (
            <i key={h} className={`h${heatLevel(n, max)}`} title={`${WEEKDAYS[d]}, ${hourLabel(h)}: ${n} ${n === 1 ? "call" : "calls"}`} />
          ))}
        </div>
      ))}
      <div className="us-hrow us-axis" aria-hidden="true">
        <span className="us-wd" />
        {Array.from({ length: 24 }, (_, h) => <span key={h}>{h % 6 === 0 ? hourLabel(h).replace(" ", "").toLowerCase() : ""}</span>)}
      </div>
    </div>
  );
}

function Bars({ v }: { v: Usage }) {
  const s = stacks(v.daily, v.purposes, BAR_H);
  const n = s.length;
  const w = 10;
  const gap = n > 14 ? 2 : 4;
  return (
    <div className="us-bars">
      <svg viewBox={`0 0 ${n * (w + gap)} ${BAR_H}`} preserveAspectRatio="none" role="img" aria-label="Tokens per day by purpose">
        {s.map((d, i) => (
          <g key={d.day}>
            <title>{`${dayLabel(d.day)}: ${short(d.total)} tokens`}</title>
            {d.parts.map((p) => (
              <rect key={p.purpose} x={i * (w + gap) + gap / 2} y={p.y} width={w} height={p.h} rx={1.5} fill={SERIES[v.purposes.indexOf(p.purpose) % SERIES.length]} />
            ))}
          </g>
        ))}
      </svg>
      <div className="us-bax" aria-hidden="true">
        <span>{dayLabel(s[0].day)}</span>
        {n > 1 && <span>{dayLabel(s[n - 1].day)}</span>}
      </div>
      {v.purposes.length > 0 ? (
        <ul className="us-legend">
          {v.purposes.map((p, i) => (
            <li key={p}><i style={{ background: SERIES[i % SERIES.length] }} />{purposeLabel(p)}</li>
          ))}
        </ul>
      ) : (
        <p className="us-n">These calls did not report token counts.</p>
      )}
    </div>
  );
}
