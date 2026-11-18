# bleep on WhatsApp: channel spec

Status: draft for approval · 18 Nov 2026 · owner: Aviroop

bleep lives on the Mac today. This spec adds WhatsApp as a second way to reach the same bleep: the same chat, the same memory, every tool, and the same approval gate. When I am away from the Mac, bleep texts me first on WhatsApp, the way Instinct does, and I can answer, approve, send a voice note or a photo from my phone.

Architecture diagram: [`whatsapp-architecture.html`](whatsapp-architecture.html) (open in a browser). Background and earlier decisions: [`build-plan.md`](build-plan.md).

---

## 1. Why

- **The moment:** bleep pings me about a bill, a meeting or a Claude Code session that waits on me. I am not at my desk. The droid bubble waits on a screen nobody looks at.
- **The fix:** bleep follows me. At the desk it talks through the droid. Away from the desk it talks through WhatsApp. One brain, two surfaces.
- **Why WhatsApp:** it is the app I already open 50 times a day. No new app, no new habit. Instinct, Poke and Pally all prove that a text thread is a good home for a personal agent.
- **Why "think like Instinct":** Instinct's best ideas are a single continuous thread, texting first when something matters, doing the job and reporting back in one short message, and voice. bleep already has the brain for this (G2, G3). WhatsApp gives it the reach.

## 2. Goals

- **W1. One chat, everywhere:** WhatsApp is a window into the one bleep chat (G2). Same session, same `messages` table, same compaction and memory. What I said on the Mac, bleep knows on WhatsApp, and the other way round.
- **W2. Every tool:** every tool the Mac chat has (Gmail, Calendar, Drive, Files, Reminders, Memory, remote MCP servers, `files_run`) works from WhatsApp with no change to the tools.
- **W3. Every feature:** approvals, proactive pings, check-ins, meeting alerts, reminders, agent watch (approve Claude Code and Codex asks, reply to idle sessions), voice, images and files.
- **W4. Texts first, calmly:** when I am away from the Mac, proactive output goes to WhatsApp. When I am at the Mac, it stays on the droid. Never both for the same thing.
- **W5. Same safety:** the approval gate decides, not the channel. A card on WhatsApp shows the same exact payload as the card on the Mac. The first answer wins.
- **W6. Feels native:** short messages, typing indicator while it works, polls or replies for approvals, voice notes back when I send voice.
- **W7. Works for other bleep users:** bleep is open source. Setup must be possible with a spare number and no Meta business account.

## 3. Non-goals (for now)

- **A cloud brain:** bleep stays local (build plan section 3). No server runs the model or holds my mail. If the Mac sleeps, bleep sleeps (section 10 says how we soften that).
- **Group chats:** bleep answers only me, in a 1:1 chat. Groups are ignored.
- **Reading my personal WhatsApp chats:** that is a separate connector (section 14, Later).
- **Voice calls:** WhatsApp calls are not possible through a linked device. Section 14 has a later path.
- **Other people talking to bleep:** one owner. Every other sender is dropped before the model sees it.

## 4. Anti-goals (fail even if it works)

- **Double pings:** the same bill on the droid and on WhatsApp. One thing, one surface.
- **A chatty bot:** five messages for one answer, raw tool logs in the thread, "Thinking..." spam.
- **A phone that approves anything:** a stolen unlocked phone must not be able to run `git push --force` or a shell command.
- **A ban on my own number:** my personal WhatsApp account is never at risk. bleep uses its own number.
- **A second history:** two chats that drift apart. WhatsApp and the Mac must show the same story.

## 5. The transport decision

There are three ways to put a bot on WhatsApp. This is the main decision in this spec.

| | A. Cloud API (official) | B. Linked device on bleep's own number (recommended) | C. Linked device on my personal number |
|---|---|---|---|
| **How** | Meta Business account, a business number, webhooks to a public URL | bleep's own number in the WhatsApp Business app on my phone. bleep joins it as a linked device (like WhatsApp Web) | bleep joins my own account as a linked device and talks in "Message yourself" |
| **Allowed for an AI assistant?** | **No, outside the EU.** Meta's "AI Providers" terms (in force since 15 Jan 2026) ban general-purpose AI assistants on the Business API. An EU interim order (9 Jun 2026) reopened it in the EU only, under appeal. I am in India | Unofficial client, against WhatsApp's terms. Risk: Meta bans **bleep's number**, not mine | Unofficial client. Risk: Meta bans **my own number** |
| **Public URL** | Needed (webhook). Breaks D7 "no server" | Not needed. The Mac connects out | Not needed |
| **Cost** | Service replies are billed since Oct 2026 above a free tier; pings outside the 24 h window need paid templates | A spare SIM or eSIM. No per-message fee | Free |
| **Proactive pings** | Only through approved templates outside the 24 h window | Any time | Any time |
| **Buttons** | Reply buttons and lists | No buttons. Polls, quoted replies and reactions | Same as B, but polls to yourself feel odd |
| **Voice notes, images, files** | Yes | Yes | Yes |
| **Setup for other users** | Business verification, a number, a webhook host | A spare number and a QR scan | A QR scan |
| **Feels like a contact** | Yes | **Yes: "bleep" is a contact with its own avatar** | No, it is me talking to me |

**Decision:** **B**, behind a `Channel` trait so A can be added later (for EU users, or if Meta's terms change). **C is rejected:** it puts my real account at risk, and bleep would see every chat I have.

**What B needs:**
- **A number for bleep:** a cheap prepaid SIM or eSIM, verified once by SMS.
- **The WhatsApp Business app** on my iPhone, registered to bleep's number, with bleep's name and the droid as the avatar. It runs next to my normal WhatsApp. I mute its notifications. A spare Android phone also works.
- **The primary phone online now and then:** WhatsApp logs out a linked device if the primary phone stays offline for about 14 days.

**Why not risk-free:** B uses a reverse-engineered protocol. The same path is used by OpenClaw and by the mautrix bridges, and Meta mostly bans bulk senders and spam. bleep sends a few messages a day to one person, inside the ping budget. If bleep's number is banned, I lose bleep's WhatsApp, not mine. The settings page says this in plain words before the QR code shows.

## 6. Architecture

Diagram: [`whatsapp-architecture.html`](whatsapp-architecture.html).

```
bleep.app (Tauri, one process)
├── Rust core
│   ├── ChatService (unchanged brain) ◀── Inbox (queue, debounce, commands)
│   │        │                                  ▲
│   │        ▼ ChatEmit                         │ InboundMsg
│   ├── Channels ── Hub: routes each turn's reply to the channel it came from
│   │     ├── panel     (the existing chat panel, now a Channel)
│   │     └── whatsapp  ── sidecar manager ──stdio JSON lines──▶ bleep-wa (Go, whatsmeow)
│   │                                                               │ WebSocket, Signal protocol
│   │                                                               ▼
│   │                                                        WhatsApp servers ──▶ my phone
│   ├── Presence ── Router: droid bubble or WhatsApp, per ping (section 9)
│   ├── Approvals (unchanged gate) ── cards fan out to panel + WhatsApp, first answer wins
│   ├── Agent watch, Reminders, Scheduler, Mail notifier ── all ping through the Router
│   ├── Media: voice in (Opus → Moonshine), voice out (say → Opus), images, files
│   └── Store: + channel columns, wa_links, wa_outbox, wa_seen
└── Web views: Settings > Channels (QR link, number allowlist, policies), channel badges in chat
```

### 6.1 The sidecar: `bleep-wa`

- **What:** a small Go binary built on [whatsmeow](https://github.com/tulir/whatsmeow), shipped inside the app bundle as a Tauri sidecar, next to `bleep-hook`.
- **Why Go and whatsmeow:** it is the most complete and best maintained WhatsApp multi-device library (it powers the mautrix-whatsapp bridge). It has media upload and download, voice notes, reactions, edits, polls with vote decryption, typing state, read receipts and QR or phone-code pairing.
- **Rust option:** the `whatsapp-rust` crate is a Rust port of the same protocol. It would avoid a second language, but it is younger. The spike (phase W0) tries both and picks one. The rest of this spec does not change with the choice, because Rust talks to it through the same JSON protocol.
- **Protocol:** JSON lines on stdin and stdout, the same pattern bleep uses for the Codex app-server. Requests carry an `id`; events carry a `type`.

```jsonc
// Rust → sidecar
{"id":1,"op":"pair","mode":"qr"}                      // events: qr (each code), paired, error
{"id":2,"op":"send_text","to":"<jid>","text":"...","quote":"<msg id>"}
{"id":3,"op":"send_poll","to":"<jid>","question":"...","options":["Approve","Decline"]}
{"id":4,"op":"send_media","to":"<jid>","kind":"voice|image|document","path":"...","caption":"..."}
{"id":5,"op":"edit","to":"<jid>","msg":"<msg id>","text":"..."}
{"id":6,"op":"react","to":"<jid>","msg":"<msg id>","emoji":"✅"}
{"id":7,"op":"typing","to":"<jid>","on":true}
{"id":8,"op":"mark_read","to":"<jid>","msgs":["..."]}
{"id":9,"op":"logout"}
// sidecar → Rust
{"type":"message","id":"...","from":"<jid>","ts":1763450000,"text":"...","quoted":"<msg id>","forwarded":false,
 "media":{"kind":"voice","path":"~/.bleep/whatsapp/media/...ogg","mime":"audio/ogg; codecs=opus","seconds":7}}
{"type":"poll_vote","poll":"<msg id>","from":"<jid>","options":["Approve"]}
{"type":"reaction","msg":"<msg id>","from":"<jid>","emoji":"👍"}
{"type":"receipt","msgs":["..."],"kind":"read"}
{"type":"state","state":"connected|connecting|logged_out|banned"}
{"type":"result","id":2,"ok":true,"msg":"<msg id>"}
```

- **Storage:** the sidecar keeps its keys in `~/.bleep/whatsapp/session.db` (whatsmeow's SQLite store), mode 600. Phase W5 encrypts it with a key from the Keychain.
- **Life cycle:** the Rust side starts it at launch when the channel is on, restarts it with backoff if it dies, and stops it on quit. Media files older than 7 days are deleted.
- **Dev build:** "bleep (dev)" uses `~/.bleep-dev/whatsapp/` and needs its own link. It must never share a session with production.

### 6.2 The `Channel` trait

The chat panel becomes one channel among others. iMessage or Telegram later are new adapters, not rewrites.

```rust
#[async_trait]
pub trait Channel: Send + Sync {
    fn id(&self) -> ChannelId;                                 // Panel, WhatsApp
    fn status(&self) -> ChannelStatus;                         // off, linking, connected, logged_out, banned
    async fn send(&self, out: Outbound) -> Result<MsgRef>;     // text, card, media
    async fn update(&self, r: &MsgRef, out: Outbound) -> Result<()>; // edit a card after a decision
    async fn typing(&self, on: bool);
    fn caps(&self) -> Caps;                                    // streaming, buttons, polls, voice, max_len
}

pub struct InboundMsg { channel: ChannelId, ext_id: String, text: String, quoted: Option<String>,
                        attachments: Vec<Attachment>, forwarded: bool, at: DateTime<Utc> }
```

### 6.3 Inbox: from a WhatsApp message to a chat turn

1. **Allowlist:** only the owner's number(s) from settings pass. Anything else is logged as "dropped" in Activity and never reaches the model. Groups, status updates and channels are dropped.
2. **Dedup:** WhatsApp can deliver a message twice after a reconnect. `wa_seen` keeps the message ids.
3. **Commands:** a message that starts with `/` is handled in code, with no model call (section 8.4).
4. **Quoted replies:** a reply to an approval card or poll is an answer to that card (section 8.2). A reply to an agent-watch message goes to that coding session (section 8.6). Everything else is chat.
5. **Media:** voice notes become text (section 8.5). Images and files are saved to `~/.bleep/inbox/` and attached.
6. **Debounce:** people send three short messages in a row on a phone. The inbox waits 2 s after the last message (max 8 s) and joins them into one turn.
7. **Queue:** `ChatService::start_turn` returns `Busy` today if a turn is running. The inbox holds new messages and sends them as the next turn when the current one ends. `/stop` cancels the running turn.
8. **Turn:** `ChatService::send_from(origin, text, attachments)`. `origin` is stored on the user message and on the reply, so the Hub knows where to answer.

### 6.4 Outbox: from a chat turn to WhatsApp

- **No streaming on WhatsApp.** While the turn runs, the sidecar shows "typing…" (refreshed every 10 s). The reply goes out once, when `AgentEvent::Done` arrives.
- **Slow turns:** if a turn passes 20 s with no text, bleep sends one short line ("On it, checking your mail and calendar.") built from the tool steps so far. Never more than one.
- **Tool steps:** not shown on WhatsApp. The Mac panel still shows them. `/why` sends the steps of the last turn.
- **Formatting:** Markdown is turned into WhatsApp style: `**bold**` to `*bold*`, `_italic_` stays, headings become bold lines, tables become short lists, code stays in triple backticks, links stay plain. No em dashes.
- **Length:** split at paragraph breaks above 1,500 characters. More than 3 parts sends the first part plus "Full answer on your Mac, or reply /more."
- **Persona:** when a turn comes from WhatsApp, the message to the model starts with a short tag: `[via WhatsApp: reply in 1 to 4 short lines, no tables, plain links]`. It is not stored in the visible chat, like the `[scheduled]` tag today.
- **Retry:** failed sends go to `wa_outbox` and retry with backoff for 24 h. After that the item shows as "not delivered" in the panel.

## 7. One chat history (W1)

"My whole chat history" means four things.

- **Same brain:** WhatsApp turns go into the same runtime session as the Mac. There is no second session, so the model has the same context, the same compaction summary and the same memory files.
- **Same log:** every WhatsApp message is a row in `messages` with `channel = 'whatsapp'`. The Mac panel shows it in place with a small WhatsApp badge. Search (`memory_search`, FTS5) covers it.
- **Catch-up on WhatsApp:** a WhatsApp thread cannot import old messages. So:
  - **On first link:** bleep sends a welcome and a 3 to 5 line "where we left off" (from the latest compaction summary and the last 10 turns; one `small` call).
  - **`/history [n]`:** the last n messages (default 10) as one text.
  - **`/export`:** the whole chat as a PDF or HTML file, sent as a document.
- **Mirror (setting, off by default):** Mac turns can also post to WhatsApp, so the phone thread is complete. Off by default because it doubles the noise. On "away", the last Mac exchange is sent as one quoted summary the first time I message from the phone after more than 2 hours, so the phone thread never starts cold.

## 8. Every feature on WhatsApp (W2, W3)

### 8.1 Tools

- **No change to any tool.** WhatsApp only changes how text gets in and out. The runtime still sees bleep's MCP server with the same tools, through the same gate.
- **One new tool, `channel_send_file`** (`path`, `caption`): sends a file to me on the channel the turn came from. Risk `safe` for files bleep made in `~/.bleep/outbox/` (an export, a converted PDF). Risk `write` (card) for any other file on the Mac, because it leaves the Mac.
- **Images to the model:** `UserMessage` gets `attachments: Vec<Attachment>`. The Claude Code backend sends images in the stream-json input (it already does this for agent screenshots). The API backend sends them as image parts.

### 8.2 Approvals

The gate in `approvals.rs` does not change. Only the way a card is shown and answered changes.

- **Fan-out:** `ApprovalEmit` sends each new card to the panel and, when the turn came from WhatsApp or I am away (section 9), to WhatsApp.
- **WhatsApp card:** one text message with the exact payload (recipient, subject, body, event time, command), then a poll with two options: **Approve** and **Decline**. A red line warns about a new recipient, like the Mac card.
- **Other ways to answer:** a quoted reply "yes" or "no", or a 👍 or 👎 reaction on the card. An unquoted "yes" only counts if exactly one card waits.
- **First answer wins:** `Approvals::decide` already refuses a second answer. After a decision, the other surface is updated: the WhatsApp card is edited to "Approved on Mac", or the Mac card shows "Approved on WhatsApp".
- **Expiry:** a card still expires after 10 minutes. On WhatsApp that is short, so a turn from WhatsApp uses a 30 minute wait (setting).
- **Phone policy:** each risk has a phone rule in Settings > Channels.

| Action | From the phone by default |
|---|---|
| Calendar, Reminders, `memory_promote`, `gmail_send` to a known recipient, remote MCP tools set to "Ask first" | Approve with the poll |
| `gmail_send` to a new recipient | Poll, then reply with my 4-digit PIN |
| `files_run`, `channel_send_file` for a Mac file, agent asks on the `ask` list (push, deploy, delete, secrets) | **Mac only.** The phone can Decline, never Approve. Setting can allow it with PIN |

- **PIN:** set on the Mac, kept as a hash in the Keychain. The message with the PIN is deleted for everyone right after bleep reads it. Three wrong PINs lock phone approvals until I unlock them on the Mac.

### 8.3 Proactive output

Everything that pings today goes through the Router (section 9). When the Router picks WhatsApp:

| Today on the Mac | On WhatsApp |
|---|---|
| Meeting bar, 60 and 10 min before | "Standup with Priya in 10 min." plus the Meet link. Quoted reply "prep" runs the Prep me turn |
| After-meeting bubble (F1) | "Call with Priya ended. Follow up?" Poll: Draft recap, Note to memory, Skip |
| Mail ping (bill, VIP, waiting on me) | One line per item, merged inside the ping budget. Quoted "not useful" works like the button |
| Travel cards (F2) | Card text with flight, PNR, check-in and leave-by times |
| Reminders (F5) | The reminder text. Quoted "snooze 30" or "done" |
| Check-ins and the morning brief | The brief as one message, in WhatsApp style |
| Memory proposals | One poll per batch: Save all, Review on Mac |

### 8.4 Commands

Handled in code. No model call. `/help` lists them.

| Command | What it does |
|---|---|
| `/stop` | Cancel the running turn |
| `/pause` and `/resume` | Pause or resume all proactive output (the tray Pause) |
| `/quiet 2h` | WhatsApp silence for a time. Pings go to the droid or the morning brief |
| `/status` | Mac awake, runtime, plan limit, pending cards, agents waiting |
| `/agents` | Coding sessions that wait on me |
| `/history [n]`, `/export` | Section 7 |
| `/why` | Tool steps of the last turn |
| `/compact` | Compact the chat now |
| `/lock` | Turn off all phone approvals until I unlock them on the Mac |

### 8.5 Voice

- **In:** a voice note (Ogg Opus) is decoded to 16 kHz mono PCM (the `ogg` and `opus` crates) and goes through the Moonshine speech to text bleep already ships. On device, no cloud. The turn text is the transcript; the panel shows it with a mic badge.
- **Out:** if I sent a voice note, bleep answers with text and, with "Voice replies" on, also a voice note: `tts::speakable` cleans the text, macOS `say -o` writes it, and it is encoded to Ogg Opus and sent as a voice note (PTT). Max 60 s; longer answers stay text.
- **Long notes:** over 2 minutes is cut into parts for Moonshine and joined.

### 8.6 Agent watch from the phone

This is the feature most worth having on a phone.

- **Waiting asks:** when a Claude Code or Codex session waits and I am away, bleep sends: "`bleep` repo: Claude Code wants to run `npm test`." Then a poll, Approve or Deny, subject to the phone policy in 8.2. Rules still decide auto-approves (D12); the phone only answers what reaches me.
- **AskUserQuestion:** the options become a poll. My vote is the answer (`updatedInput.answers`).
- **Idle sessions:** when a session stops and waits for me, bleep sends the last assistant message (cut to 400 characters). A quoted reply goes to `AgentHub::reply` through the waiting `claude-reply` hook, the same path as the Mac panel.
- **Untrusted:** the session text is from an agent that may have read web pages. It is shown as a quote and never decides anything.

## 9. Presence and routing (W4)

The Router decides, per ping, where it goes. It replaces the direct `BarUi::bubble` and `ping` calls with `Router::deliver(Ping)`.

- **Presence signals (no model):** seconds since the last keyboard or mouse event (`CGEventSourceSecondsSinceLastEventType`), screen locked, display asleep, lid closed, and the last channel I wrote from.
- **States:**
  - **At the Mac:** input in the last 5 min and screen unlocked. Pings go to the droid.
  - **Away:** no input for 5 min, or screen locked. Pings go to WhatsApp.
  - **On the phone:** I wrote on WhatsApp in the last 15 min. Pings go to WhatsApp, even if the Mac is unlocked.
- **Escalation:** an urgent item (meeting in 10 min, agent waiting, bill due today) shown on the droid and not seen in 10 min (no click, no input) is sent to WhatsApp once.
- **Calm rules apply across channels:** one ping budget (4 an hour), one dedup table, one quiet hours window. WhatsApp quiet hours can be stricter (setting). Held items go into the morning brief.
- **Read sync:** a WhatsApp read receipt clears the droid bubble for the same item. Opening the item on the Mac marks the WhatsApp message read.

## 10. When the Mac sleeps

bleep runs on the Mac. A linked device that is offline gets its messages later, when it connects again. So nothing is lost, but nothing is answered while the Mac sleeps.

- **Keep awake (setting, on by default when the channel is on):** while on power, bleep holds a "prevent idle sleep" power assertion. No admin rights needed. A closed lid on battery still sleeps.
- **Catch-up:** on wake, the inbox reads the queued messages in order. If the oldest is more than 5 min old, the reply starts with "Sorry, my Mac was asleep." Turns that are now stale (a meeting that already started) are answered with that fact, not a prep note.
- **Last seen:** bleep's "last seen" in WhatsApp is the honest signal that it is offline.
- **Before sleep:** on the macOS "will sleep" event, bleep sends nothing by default. Option: one line, "Going offline, Mac is going to sleep," at most once in 6 hours.
- **Later (needs a decision):** a tiny always-on stub in the cloud that only says "Mac asleep, I will answer when it wakes." It would hold a WhatsApp session in the cloud, which goes against "data stays local". Not in this spec.

## 11. Security

- **Owner only:** the allowlist (my numbers, in E.164) is the first check. Messages from anyone else never reach the model, the store or a tool. A setting can notify me of unknown senders.
- **Trust labels:** my own typed text and voice are user input. Forwarded messages, quoted messages from others, documents and images are wrapped and labelled untrusted, like mail.
- **Same gate:** every tool call still passes the approval gate in Rust. The channel cannot skip it.
- **Phone loss:** the phone policy (8.2), the PIN, `/lock`, and an "Unlink WhatsApp" button on the Mac. WhatsApp's own "Linked devices" screen can also remove bleep.
- **Encryption:** WhatsApp messages are end-to-end encrypted between my phone and bleep's sidecar. Meta sees metadata, not content. Keys stay on the Mac.
- **Local files:** `~/.bleep/whatsapp/` is mode 700. Media in the inbox is deleted after 7 days. Logs never hold message text.
- **Rate:** at most 60 outgoing messages an hour (hard cap in the sidecar), on top of the ping budget. This keeps bleep far from spam patterns.
- **Ban handling:** a `banned` or `logged_out` state turns the channel off, routes everything to the droid, and shows a clear message in Settings.
- **Audit:** every inbound message, outbound message, card answer and dropped sender is in the Activity tab, with the channel.

## 12. Data changes

```
messages      + channel TEXT NOT NULL DEFAULT 'panel', + ext_id TEXT       -- WhatsApp message id
approvals     + answered_via TEXT, + ext_refs TEXT                        -- card and poll ids per channel
pings         + channel TEXT, + ext_id TEXT
agent_asks    + answered_via TEXT
wa_links      id, own_jid, owner_jids, linked_at, state, last_seen
wa_outbox     id, kind, payload, attempts, next_at, status
wa_seen       ext_id PRIMARY KEY, at                                     -- dedup, pruned after 7 days
```

`config.toml` gets a `[whatsapp]` table: `enabled`, `owner_numbers`, `away_after_min` (5), `escalate_after_min` (10), `voice_replies`, `mirror`, `phone_policy`, `quiet_hours`, `keep_awake`, `turn_wait_min` (30).

## 13. Settings > Channels

- **WhatsApp row:** status (off, linking, connected, logged out), bleep's number, last seen.
- **Link:** a plain-words note on the risk (section 5), then a QR code that refreshes, or "Link with phone number" (an 8-character code). Shows "Linked" with a test message button.
- **My number(s):** the allowlist. The first link fills it with the number that sent the first test reply, after I confirm it on the Mac.
- **Routing:** away after N min, escalate after N min, WhatsApp quiet hours.
- **Phone approvals:** the table in 8.2, with PIN setup.
- **Voice replies, Mirror, Keep awake:** switches.
- **Unlink:** logs out the sidecar and deletes `~/.bleep/whatsapp/`.

## 14. Later

- **Cloud API adapter (option A):** for EU users or if Meta's terms allow it. Needs a small relay (Vercel function plus Upstash Redis) for the webhook. The Mac pulls from the relay over an outbound connection, so bleep still opens no port. Reply buttons replace polls.
- **iMessage channel:** the same `Channel` trait. On a Mac this is local: read `~/Library/Messages/chat.db`, send with AppleScript. Needs Full Disk Access. Instinct's other home.
- **WhatsApp as a source:** a read-only connector on my personal account (option C, but read-only, opt-in), for "who is waiting on my reply" like Gmail's waiting-on-me. Big privacy cost; needs its own spec.
- **Calls:** an urgent item could ring me. Only the Cloud API has a calling API, so this waits for option A.
- **Always-on stub:** section 10.

## 15. Phases

Each phase ends with something I use every day. Plans go in `docs/superpowers/plans/` as before.

### W0. Spike (1 to 2 days)

- Pair `bleep-wa` (whatsmeow) and `whatsapp-rust` with a test number. Send and receive text, a voice note, an image, a poll and a vote, a reaction, an edit, typing.
- Confirm: polls in a 1:1 chat decrypt votes on a linked device; the WhatsApp Business app works as the primary; offline messages arrive after a Mac sleep of 8 hours.
- Pick the library. Write the result into this spec.

**Done when:** a poll vote from my phone prints in the bleep log.

### W1. Text channel

- `Channel` trait, the panel as a channel, the `bleep-wa` sidecar and manager, Settings > Channels with QR link and allowlist.
- Inbox (allowlist, dedup, debounce, queue, commands) and outbox (typing, formatting, split, retry).
- `channel` column, badges in the panel, WhatsApp persona tag, welcome catch-up, `/history`.

**Done when:** I ask "what's on my calendar tomorrow" on WhatsApp, get a short answer with no tool log, and see the same exchange in the Mac panel with a WhatsApp badge.

### W2. Approvals and the Router

- Card fan-out, polls, quoted and reaction answers, cross-surface updates, phone policy and PIN.
- Presence, Router, escalation, shared calm rules, read sync. Meeting, mail, travel, reminder and check-in pings on WhatsApp.

**Done when:** with the Mac locked, a meeting alert reaches WhatsApp and not the droid, "reply to Priya saying I'm 5 min late" shows a card with a poll, and Approve on the phone sends the mail and edits the Mac card.

### W3. Agent watch on the phone

- Waiting asks with polls, AskUserQuestion as polls, replies to idle sessions, Mac-only rule for the `ask` list.

**Done when:** I walk away from three Claude Code sessions, approve `npm test` from the phone, answer an idle session with a quoted reply, and a `git push --force` can only be declined from the phone.

### W4. Voice, images, files

- Voice in (Opus to Moonshine), voice out (say to Opus), images and documents to the model, `channel_send_file`, `/export`.

**Done when:** I send a voice note "remind me to call mom at 7", get a voice reply, and the reminder fires at 7 on WhatsApp.

### W5. Hardening

- Keep awake, sleep catch-up, ban and logout handling, session store encryption, media cleanup, rate cap, Activity entries, the dev build's separate session.

**Done when:** the Mac sleeps overnight, three messages sent at night are answered in order on wake with the "Mac was asleep" line, and Unlink leaves no files behind.

## 16. Testing

Same rules as the build plan: tests never touch WhatsApp, Google or a browser.

- **Fake sidecar:** a `FakeWa` channel and recorded sidecar JSON fixtures in `src-tauri/tests/fixtures/whatsapp/` (messages, votes, reactions, receipts, states).
- **Rust unit tests:** allowlist and drop, dedup, debounce and merge, the busy queue, commands, Markdown to WhatsApp formatting and splitting, approval answer parsing (poll, quote, reaction, bare "yes" with one or two cards), phone policy and PIN lock, Router states and escalation, shared ping budget, catch-up after sleep.
- **Contract:** the panel and WhatsApp channels pass the same `Channel` test suite.
- **Go:** `go test` for the sidecar protocol encode and decode, with whatsmeow's client behind an interface.
- **UI:** Playwright, headless, `?mock=1`, for Settings > Channels and the badges.
- **Manual:** each phase's "done when" with a real test number.

## 17. Risks

- **Ban of bleep's number:** the main risk of option B. Mitigation: a dedicated number, low volume, the rate cap, human-like typing, a clear warning at setup. The fallback is the droid, which always works.
- **Protocol breaks:** WhatsApp changes the protocol and the library needs an update. Mitigation: pin the library, a smoke test per release, update the sidecar with each bleep release.
- **Meta's terms change:** if the AI Providers ban ends, add option A.
- **Mac asleep:** answers wait. Mitigation: keep awake on power, the catch-up line, honest "last seen".
- **Phone in the wrong hands:** Mitigation: phone policy, PIN, `/lock`, Unlink.
- **Plan limits:** more turns from the phone use more of my Claude plan. Mitigation: commands and routing need no model; the `small` tier for the catch-up; the existing limits.
- **Noise:** two surfaces can double the pings. Mitigation: one Router, one budget, read sync.

## 18. Open questions

- **Number:** a new prepaid SIM, an eSIM, or a spare phone? This decides where the WhatsApp Business app runs.
- **Library:** whatsmeow (Go sidecar) or `whatsapp-rust` (no Go). Decided by W0.
- **Default for Mirror:** off as written, or on for the first week while I get used to it?
- **Phone approvals for Gmail:** is the PIN for new recipients too strict for daily use?

## Sources

- WhatsApp Business Platform pricing changes, Oct 2026: [Courier](https://www.courier.com/blog/whatsapp-pricing-changes-october-2026), [WATI](https://www.wati.io/en/blog/whatsapp-business-api-pricing-changes-2026-what-meta-s-two-new-fees-actually-cost-you/)
- Meta's AI Providers ban and the EU case: [TechCrunch, Dec 2025](https://techcrunch.com/2025/12/04/eu-investigating-meta-over-policy-change-that-bans-rival-ai-chatbots-from-whatsapp/), [Decrypt, Jun 2026](https://decrypt.co/370580/eu-orders-meta-open-whatsapp-rival-ai-chatbots), [Quartz](https://qz.com/eu-meta-whatsapp-rival-ai-chatbots-antitrust-061026)
- Instinct: [Vellum breakdown](https://www.vellum.ai/blog/official-instinct-breakdown), [eesel review](https://www.eesel.ai/blog/instinct-ai-review), [TechCrunch, Oct 2026](https://techcrunch.com/2026/10/24/all-the-ai-agents-that-can-live-in-your-text-messages/)
- whatsmeow: [github.com/tulir/whatsmeow](https://github.com/tulir/whatsmeow)
