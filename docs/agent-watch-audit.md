# Agent watch audit (19 Nov 2026)

Scope: every Claude Code transcript changed since 13 Nov (66 sessions, all projects), plus the `agent_asks` log in `~/.bleep/bleep.db`.

## Verdict

Agent watch has auto-answered **0** asks. That is expected from the current code and config. It is not a single bug. Five things stack:

1. **Auto-approve is off.** `[agents] auto_approve = false` in `~/.bleep/config.toml` and `~/.bleep-local/config.toml`. It is the default (`config.rs:66`). With it off, nothing is ever auto, the screenshot check included (`hub.rs:193`, `hub.rs:333`).
2. **The rules engine has nothing to judge.** It only approves `Bash` and `Read`-type permission prompts. Your sessions run in auto mode, so Claude Code never sends those. 0 shell permission asks in 6 days. All 9 "permission" rows are `AskUserQuestion`.
3. **bleep does not see most questions.** In the desktop app the question lives in the `wrap_up` tool call (the next-step box with buttons). bleep's `Stop` handler only looks at the last text and only when it ends in `?` (`hub.rs:322`). 143 of 158 desktop questions did not. bleep marked those sessions **Done** while they waited for you. The log has 35 asks. The transcripts have 180.
4. **The policy never says yes to a question.** D12 lets only rules approve, and rules only cover commands. A question can be auto-answered only by the screenshot path ("does this look right?" + a screenshot). None of the 180 questions were that kind.
5. **`AskUserQuestion` is hard-coded to wait for you** (`hub.rs:303`).

## What the sessions show

| | Count |
|---|---|
| Turns that ended with a question to you | 180 |
| You clicked the agent's first button, or said the same in words | 73 (41%) |
| ... low risk (start, plan, build, fix, commit, push, open PR, merge own PR when green) | ~40 |
| ... high risk (ship to Vercel, release, prod, delete, reset) | 27 |
| You redirected, added info, or judged a result by eye | ~107 |
| `AskUserQuestion` calls | 15 (about half: you took every "(Recommended)" option) |

Median time before you clicked a plain go-ahead: **1.6 min**. Total idle time on those: about **20 hours**.

### Should have been auto-answered (low risk, you always said yes)

- **Run mode and next step:** "Which way should I run the plan?" → *Native, start Task 1* (x4), *Approve, run Native* (x3), *Write the plan*, *Approve, start*, *Yes, start it*.
- **CI loops:** "Merge PR 19 once CI is green?", "Merge PR 27 once CI is green?", "Merge #42 with --admin once CI passes?", "Check #45 again and continue?", "Check #46 and merge it when green?" (asked 3 times in a row), "Merge PR #25?" (you wrote "merge when CI pass" twice). Your memory already says: merge own PRs with `--admin`.
- **Push and PR:** "Push this branch and open a PR so CI runs?" (x2), "Push this to PR 27?", "Push the README change?", "Commit this and open a PR?".
- **Small follow-ups the agent proposed:** "Fix Editor.tsx to wait for fonts?", "Redraw the favicon with Puff too?", "Add a global theme option?", "Rename the Apple payments?", "Save the RAG discussion to the notes?".

### Correctly left to you

- **Taste and eyes:** "How does it feel on your music now?", "Does the bulb follow the video?", "What's wrong or missing here?", "Which buddy should replace the droid?".
- **Prod and irreversible:** "Ship ... to Vercel?" (13 times, always *Ship it*), "Cut 0.3.1?", "Clear the 7.9 GB cache?", "Reset the Docker data?".
- **Questions with no buttons** where you added new facts.

## Adjustments

Ordered by impact. 1 and 2 are needed before anything else can work.

**Status:** 1 to 3 are built on branch `feat/agent-auto-answer` (`agentwatch/answer.rs`, `transcript.rs`, `hub.rs`, Settings > Agents > Answers). The default mode is "Log only".

1. **See the real question.** In `transcript.rs`, read the last `mcp__ccd_turn__wrap_up` tool call of the turn: its `ask` and `actions[].label`. Also take a question from inside the final message, not only its last character. Fix the state: a turn with an open ask is **Waiting**, not Done. Effect: bleep sees ~180 asks, not 35.
2. **Add an "answer for me" policy for questions.** Keep D12's spirit: rules pick what may be answered, a model may only veto.
   - **Reply text is the agent's own first button label.** bleep never writes free text into the agent. No buttons, no auto.
   - **Rules classify the ask + label** with two new lists in `agent-rules.toml`:
     - `answer_auto`: start, plan, run native, build, fix, add, rename, save, commit, push, open PR, check CI, merge when green.
     - `answer_ask` (always wins): ship, deploy, release, prod, live, Vercel, public, delete, clear, reset, undone, install, pay, plus "feel", "look", "test first", "which" with no recommended option.
   - **Small-model veto:** given the ask, the label, the last user prompt and your CLAUDE.md "Git" rules, "is this a plain go-ahead on work you already asked for?" A no or low confidence goes to you. It can never turn a rule "ask" into a yes.
   - **Grace window:** ping first ("Answering *Merge it* in 60 s. Tap to stop."), then answer through the existing `claude-reply` hook. Your median reply was 1.6 min, so 60 s keeps you in control.
   - **Loop guard:** at most 5 auto answers per session per hour. Stop on the third identical ask (the "Check #46 again" loop).
3. **Shadow mode first.** Run the policy for a week with answers logged, not sent. On the next `UserPromptSubmit`, store what you actually typed on the expired ask. Turn it on when it matches you on the safe class. This also fixes the log: today 30 of 35 rows say only "expired".
4. **Make the switch visible.** Ask about auto-answer during setup, and show "Auto-answer is off" in the Agents view. Today it is off with no sign.
5. **`AskUserQuestion`:** keep it manual for now. Add a one-tap "Take recommended" button on the card. Auto-pick only later, and only when every question has a "(Recommended)" option and passes the same rules.
6. **Cut the asks at the source (cheap, today).** Add to `~/.claude/CLAUDE.md`: "After opening a PR, watch CI and merge it when green. Do not ask. Do not ask which run mode: use Native in this session. Do not ask before writing a plan you proposed." That removes ~15 of the 40 safe asks with no code.
7. **Opt-in prod trust (later).** You said *Ship it* to every Vercel landing deploy. A per-project `ship_auto = ["landing/"]` list could cover that. Keep it off by default.

## Expected result

With 1 to 3 and 6: about **40 of 180 waits** (22%) answered without you, roughly half of the 20 idle hours gone, and nothing risky sent without a tap.

Method: a script paired each turn that ended with a question with your next typed message, then matched the reply to the agent's button labels. The script is not committed.
