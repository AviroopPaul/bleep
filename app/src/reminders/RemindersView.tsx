import { useCallback, useEffect, useState } from "react";
import { api, onReminders, type ReminderItem, type RepeatKind } from "../ipc";
import { defaultAt, group, REPEATS, repeatLabel, SNOOZES, whenLabel } from "./remindersText";

/** The reminder list, kept fresh by `reminders://changed`. Also gives the tab its count. */
export function useReminders() {
  const [list, setList] = useState<ReminderItem[]>([]);
  const refresh = useCallback(() => api.remindersList().then(setList).catch(() => {}), []);
  useEffect(() => {
    void refresh();
    const un = onReminders(() => void refresh());
    // Times like "Today" roll over; a light refresh keeps the labels right.
    const t = window.setInterval(() => void refresh(), 60_000);
    return () => {
      void un.then((f) => f());
      window.clearInterval(t);
    };
  }, [refresh]);
  return { list, refresh };
}

const XIcon = () => (
  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" /></svg>
);

export function RemindersView({ list, refresh }: { list: ReminderItem[]; refresh: () => void }) {
  const [text, setText] = useState("");
  const [at, setAt] = useState(() => defaultAt(new Date()));
  const [repeat, setRepeat] = useState<RepeatKind>("none");
  const [err, setErr] = useState<string | null>(null);
  const now = new Date();
  const g = group(list);

  const act = (p: Promise<unknown>) => p.then(() => (setErr(null), refresh())).catch((e) => setErr(String(e)));
  const add = (e: React.FormEvent) => {
    e.preventDefault();
    if (!text.trim()) return;
    void api
      .reminderAdd(text.trim(), at, repeat)
      .then(() => {
        setText("");
        setRepeat("none");
        setAt(defaultAt(new Date()));
        setErr(null);
        refresh();
      })
      .catch((e) => setErr(String(e)));
  };

  const meta = (r: ReminderItem) => [whenLabel(r.next, now), repeatLabel(r.repeat), r.snoozed ? "Snoozed" : ""].filter(Boolean).join(" · ");

  return (
    <div className="rems" role="tabpanel" aria-label="Reminders">
      <form className="rm-add" onSubmit={add}>
        <label htmlFor="rm-text" className="sr-only">Remind me to</label>
        <input id="rm-text" className="rm-text" placeholder="Remind me to..." value={text} maxLength={200} onChange={(e) => setText(e.target.value)} />
        <div className="rm-when">
          <label htmlFor="rm-at" className="sr-only">When</label>
          <input id="rm-at" type="datetime-local" value={at} onChange={(e) => setAt(e.target.value)} />
          <label htmlFor="rm-rep" className="sr-only">Repeat</label>
          <select id="rm-rep" value={repeat} onChange={(e) => setRepeat(e.target.value as RepeatKind)}>
            {REPEATS.map((r) => <option key={r.value} value={r.value}>{r.label}</option>)}
          </select>
          <button type="submit" className="btn pri" disabled={!text.trim()}>Add</button>
        </div>
      </form>
      {err && <p className="rm-err" role="alert">{err}</p>}

      {list.length === 0 && (
        <p className="ag-empty">No reminders yet. Add one here, or ask bleep "remind me at 6 to call mom".</p>
      )}

      {g.due.length > 0 && (
        <section aria-labelledby="rm-due-h">
          <h2 className="ag-h" id="rm-due-h">Due now</h2>
          <ul className="ag-list">
            {g.due.map((r) => (
              <li key={r.id} className="rm-row due">
                <div className="rm-main">
                  <span className="rm-title">{r.text}</span>
                  <span className="rm-meta">{whenLabel(r.due, now)}</span>
                  <div className="rm-acts">
                    <button className="btn pri sm" onClick={() => void act(api.reminderDone(r.id))}>Done</button>
                    {SNOOZES.map((s) => (
                      <button key={s.minutes} className="btn sm" onClick={() => void act(api.reminderSnooze(r.id, s.minutes))}>{s.label}</button>
                    ))}
                  </div>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}

      {g.upcoming.length > 0 && (
        <section aria-labelledby="rm-up-h">
          <h2 className="ag-h" id="rm-up-h">Upcoming</h2>
          <ul className="ag-list">
            {g.upcoming.map((r) => (
              <li key={r.id} className="rm-row">
                <div className="rm-main">
                  <span className="rm-title">{r.text}</span>
                  <span className="rm-meta">{meta(r)}</span>
                </div>
                <button className="rm-x" aria-label={`Delete ${r.text}`} title="Delete" onClick={() => void act(api.reminderDelete(r.id))}><XIcon /></button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {g.done.length > 0 && (
        <section aria-labelledby="rm-done-h">
          <h2 className="ag-h" id="rm-done-h">Done</h2>
          <ul className="ag-list">
            {g.done.map((r) => (
              <li key={r.id} className="rm-row done">
                <div className="rm-main">
                  <span className="rm-title">{r.text}</span>
                  <span className="rm-meta">{whenLabel(r.due, now)}</span>
                </div>
                <button className="rm-x" aria-label={`Delete ${r.text}`} title="Delete" onClick={() => void act(api.reminderDelete(r.id))}><XIcon /></button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
