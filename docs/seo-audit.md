# bleep SEO and GEO audit

Date: 18 Nov 2026. Scope: `landing/index.html` (local and live at `bleep-omega.vercel.app`), the GitHub repos, and bleep's presence on the web.

Goal: Google, ChatGPT, Perplexity, Gemini and Claude recommend bleep when people ask for a Grok Bot, OpenMausBot, OpenBot or OpenClaw alternative, and directories such as opensourcealternatives.to list it.

---

## Status (18 Nov 2026)

- **Done:** repo public under MIT; GitHub description, 20 topics, private vulnerability reports; README, CONTRIBUTING, SECURITY, issue and PR templates; current screenshots and demo GIF in `docs/media/`.
- **Done on the site:** T2, T3, T4, T5, T6, T7 (partly), T9, T10 (summary), T11 (in JSON-LD and pages), T13, T16; new pages from section 4 (`scripts/seo/pages.py`).
- **Open:** D2 domain (T1), Search Console and Bing, real screenshots on the home page (T12), notarization, all of section 5.

---

## 1. Summary

- **bleep is invisible today.** A web search for bleep as a Mac assistant returns nothing. It returns CC-Beeper, Buddi and ClaudePet (Claude Code companions) and four unrelated Mac apps named "Bleep".
- **Two blockers come before any page fix.**
  - **The source is private.** opensourcealternatives.to accepts "public repositories on github.com only". openalternative.co also lists only open source tools. Every "open source Grok Bot alternative" list ranks by GitHub stars. With a private repo, bleep cannot enter the channel you named.
  - **There is no own domain.** `bleep-omega.vercel.app` has no authority, looks temporary, and every link earned now is lost when you move.
- **The page has no machine-readable signals.** No `robots.txt`, no `sitemap.xml`, no JSON-LD, a title and H1 with no search terms, and H2s like "Questions." that say nothing out of context.
- **What drives AI recommendations is mostly off-site.** Research says best-of listicles are about 44% of cited sources in ChatGPT product answers, earned media dominates AI citations, and Reddit presence was one of the strongest predictors in one test. Page fixes are needed, but they are the smaller half.

---

## 2. Market research: who we compete with

### 2.1 The names you gave

| Product | What it is | Price / access | Source |
|---|---|---|---|
| **Grok Bot** (xAI) | Persistent AI "teammates". Each bot has a cloud Linux VM, signs in to your apps, keeps memory. Beta since 11 Aug 2026. | Needs SuperGrok Heavy, Cursor Ultra or Cursor Teams Premium. Enterprise waitlist. | x.ai, VentureBeat, eesel |
| **OpenMausBot** | Open source Grok Bot alternative. A desktop app for a team of bots on Claude, Codex or Grok Build CLIs, each with its own VM. ~24k stars. | Free. Uses the CLI subscriptions you already have. | GitHub, openmausbot.vercel.app |
| **OpenBot** | Open source. Per-agent containers with Chromium, a policy-gated audit trail. ~12k stars. Aimed at teams. | Free, self-hosted. | opensourcealternatives.to |
| **OpenClaw** (ex Clawdbot) | Open source personal assistant in the chat apps you use. ~395k stars. The default answer to "personal AI assistant". | Free, needs model API. | Wikipedia, MacStories |

Other names that appear in every list: rakazo, Hermes Agent, openhuman, paperclip, Buzz, NanoClaw, IronClaw, Vellum, LittleBird.

### 2.2 The honest position for bleep

bleep is **not** a cloud agent team like Grok Bot. It does not run VMs or click through other apps. If the page claims it is a full Grok Bot replacement, list writers and LLMs will see the gap, and the claim will not hold.

bleep **does** own a slot no one else holds:

- **Proactive, not on demand.** It watches your mail, calendar and coding agents and taps you. Grok Bot, OpenMausBot and OpenClaw wait for you to ask.
- **Lives on your Mac.** Native, small Rust core, no bleep server. Grok Bot and OpenMausBot run cloud machines.
- **Uses the Claude Code plan you already pay for.** No API key, no new bill. OpenMausBot has this angle too, so do not lead with it alone.
- **Approves your coding agents safely.** Safe asks pass, risky ones wait. This beats CC-Beeper, Buddi and ClaudePet, which only notify.
- **Personal life plus dev work in one buddy.** Bills, meetings, mail and Claude Code / Codex in one place.

**Positioning line (proposed):** "bleep is a free, open source AI assistant that lives on your Mac. It watches Claude Code, Codex, Gmail and Calendar, clears the small asks, and taps you only when it matters. A local alternative to Grok Bot and OpenClaw that runs on the Claude plan you already have."

Use this sentence, word for word, on the page, the README, every directory and every launch post. LLMs learn an entity from repeated, consistent descriptions across sources.

### 2.3 Search terms to target

| Cluster | Example queries | Where to win |
|---|---|---|
| Alternatives (high intent) | grok bot alternative, open source grok bot alternative, openclaw alternative mac, openmausbot alternative | Comparison pages, directory listings, listicles |
| Category | ai assistant for mac, proactive ai assistant, personal ai assistant mac free, desktop ai buddy | Home page title, H1, intro |
| Claude Code companion | claude code notifications mac, auto approve claude code permissions, claude code companion app, monitor claude code sessions | A guide page, README, r/ClaudeAI |
| Problem | ai that reminds me of bills from gmail, meeting prep assistant mac | Use case sections, blog posts |

The Claude Code companion cluster is the easiest win. Competition there is a few small repos (CC-Beeper has 175 stars), and the audience matches bleep's install base exactly.

---

## 3. Technical and on-page audit

Severity: **P0** blocks visibility. **P1** large effect. **P2** worth doing.

### 3.1 Findings

| # | Sev | Finding | Fix |
|---|---|---|---|
| T1 | P0 | Site lives on `bleep-omega.vercel.app`. | Buy a domain. `bleep.app` and `bleep.dev` are parked for sale (Afternic). `getbleep.app` is taken. `usebleep.app`, `trybleep.app`, `bleepbuddy.app` had no DNS on 18 Nov (check before buying). Point it at Vercel, 308 redirect the old host, keep `/install.sh` working on both. Update the Homebrew cask, the app's update URL and install script. |
| T2 | P0 | No `robots.txt`. | Add one that allows all, names the sitemap, and explicitly allows GPTBot, OAI-SearchBot, ChatGPT-User, ClaudeBot, Claude-SearchBot, PerplexityBot, Google-Extended, Applebot-Extended. (Live check: these bots get a 200 today, so nothing blocks them. This makes intent explicit.) |
| T3 | P0 | No `sitemap.xml`. | Add one, list every page with `lastmod`. Submit in Google Search Console and Bing Webmaster Tools. |
| T4 | P1 | Title "bleep: a little buddy for your Mac" has no search term. | "bleep: Free AI Assistant for Mac, Built on Claude Code" (54 chars). After open sourcing: "bleep: Open Source AI Assistant for Mac". |
| T5 | P1 | H1 is "Meet bleep". | Keep the visual. Make the H1 text "Meet bleep, the AI buddy for your Mac" or put the descriptive line inside the H1. One H1 that names the category. |
| T6 | P1 | No structured data. | Add JSON-LD: `SoftwareApplication` (name, `operatingSystem: "macOS 15 or later"`, `applicationCategory: "ProductivityApplication"`, `offers` price 0, `downloadUrl`, `softwareVersion`, `datePublished`, `author`, `sameAs` to GitHub), `FAQPage` from the existing FAQ, `Person` for you with `sameAs` (GitHub, LinkedIn, site), `WebSite`. Google says markup is not an AI lever, but it is a clean entity signal for Bing and LLM crawlers. Keep it in step with visible text. |
| T7 | P1 | H2s are slogans: "A day with bleep.", "Plugs into your day.", "Questions.". LLMs and AI Overviews retrieve passages by heading. | Keep the slogan, add a plain eyebrow or subhead that says the fact: "Live demo of the bleep Mac assistant", "Integrations: Claude Code, Codex, Gmail, Google Calendar, Drive, MCP", "bleep FAQ". |
| T8 | P1 | One page only. Nothing can rank for "grok bot alternative" or "openclaw alternative". | Add pages (section 4). |
| T9 | P1 | The words "open source", "AI assistant", "personal assistant", "Grok Bot", "OpenClaw" never appear on the page. | Add a short "How bleep compares" block on the home page with a 4-row table, and link to the full pages. |
| T10 | P1 | Demo mock text (terminal lines, fake bills) is the first ~250 words a crawler reads after the H1. It dilutes the page summary. | Add a 2 to 3 sentence plain summary right under the H1 (the positioning line). Mark the decorative mock blocks `aria-hidden="true"` where they are not interactive. |
| T11 | P2 | No visible version or "updated" date. Freshness is a ranking and citation factor. | Show "Version 0.4.0 · Updated Nov 2026" near the install box, and keep it in the JSON-LD. Add a `/changelog` page. |
| T12 | P2 | One `<img>` on the page. The demo is CSS. Image search and AI answers that show images get nothing. | Add 3 to 4 real screenshots (`brand/`, `mocks/` already have assets) with descriptive alt text and file names like `bleep-mac-assistant-approves-claude-code.png`. |
| T13 | P2 | No `/privacy` page. Privacy is a selling point and a trust signal, and Google OAuth verification needs one. | Add `/privacy` with the "no bleep server" facts. |
| T14 | P2 | `og:image` is fine. No `twitter:site` / `twitter:creator`. | Add your X handle if you have one. |
| T15 | P2 | Install by `curl ... | sh` from a vercel.app host can look unsafe to reviewers. | After T1, serve from your domain. Say in the FAQ that the script checks the SHA-256 and that the source is public. (The app is signed with bleep's own certificate, not notarized by Apple. Notarization needs a paid Apple Developer account and would remove a trust objection.) |
| T16 | P2 | `llms.txt` missing. | Optional. 2026 data (Ahrefs, ~137k domains) says 97% of these files get zero requests and no study shows a citation lift. Google says ignore it. Cost is 10 minutes, so add a short one, but do not expect results. |
| T17 | OK | HTTPS, HSTS, 200 to AI bots, gzip, `lang="en"`, canonical, OG and Twitter tags, viewport. | Keep. Update canonical and OG URLs after T1. |
| T18 | OK | Page weight ~159 KB transferred, one HTML file, web fonts preconnected. | Fine. Check Core Web Vitals in Search Console after launch. |

### 3.2 GitHub (counts as a search surface)

- **`AviroopPaul/bleep` is private, 0 stars, no topics.** See decision D1.
- **`AviroopPaul/bleep-releases` is public, 0 stars, no topics.** Its description says "Source is private", which disqualifies it in every open source directory.
- Fix now, even before D1: add topics (`macos`, `ai-assistant`, `personal-assistant`, `claude-code`, `codex`, `mcp`, `tauri`, `gmail`, `google-calendar`, `grok-bot-alternative`, `openclaw-alternative`), a README with the positioning line, a GIF, the install command and a comparison table, and set the homepage to the new domain.

---

## 4. Content to add (code work)

Static HTML files in `landing/`, same style as the home page. Add `"cleanUrls": true` in a `landing/vercel.json`.

| Page | Purpose | Notes |
|---|---|---|
| `/vs/grok-bot` | Rank and be cited for "grok bot alternative". | Honest table: where it runs, price, needs, proactive or not, approvals, memory, open source. Say plainly what Grok Bot does that bleep does not (cloud VMs, operating any app). Honest pages get cited. One-sided pages do not. |
| `/vs/openclaw` | "openclaw alternative mac". | OpenClaw lives in chat apps and needs an API key. bleep floats on the Mac, is proactive, uses Claude Code. |
| `/vs/openmausbot` | "openmausbot alternative". | Both use your CLI plan. OpenMausBot is an agent team in VMs. bleep is one proactive buddy on the desktop. |
| `/alternatives` | Hub: "Grok Bot, OpenClaw and OpenMausBot alternatives for Mac". | Links all `/vs/` pages. List CC-Beeper, Buddi too. A fair hub is the kind of page LLMs quote. |
| `/claude-code-companion` | Own the Claude Code cluster. | "Get notified and auto-approve safe Claude Code permission prompts on Mac." How the rules work, with the default allow list. |
| `/changelog` | Freshness, gives crawlers a reason to come back. | Generate from release notes. |
| `/privacy` | Trust, OAuth. | See T13. |

Each page: own title, description, canonical, H1, JSON-LD (`SoftwareApplication` reference plus `BreadcrumbList`), a dated "Updated" line, and links back to home and install.

Content rules that the research supports (Princeton GEO study): concrete numbers, direct quotes, and cited sources raise inclusion in AI answers by roughly 30 to 40%. So state facts with numbers: "approves 4 kinds of safe asks", "under X MB RAM", "0 servers", "macOS 15+, Apple silicon". Measure the real RAM and binary size first.

---

## 5. Off-site plan (not code)

This is where most of the result comes from. Do it in this order.

### Phase 0: Foundations (week 1)

1. **Decide D1 and D2** (section 7).
2. **Google Search Console and Bing Webmaster Tools.** Verify the new domain, submit the sitemap, request indexing. ChatGPT search draws heavily from Bing results (one study: 87% of SearchGPT citations matched Bing top results), so Bing matters as much as Google. Turn on IndexNow in Bing.
3. **Write one canonical description** (the positioning line) and one 50-word and one 150-word version. Reuse them everywhere.

### Phase 1: Directories (weeks 1 to 2, after the repo is public)

| Directory | Why | Notes |
|---|---|---|
| **opensourcealternatives.to/submit** | Your named target. ~210k visits a month, 68% from Google. Has a "Grok Bot" alternatives guide and a "Claude Code" page. CC-Beeper is listed. | Needs project name, email, website, public github.com repo. Then email the editor (guide is "By Suraj") to ask for a review for the Grok Bot and Claude Code pages. |
| **openalternative.co** | Has "AI Personal Assistants" (22 tools), "Claude Code Alternatives" (19), a Grok Bot page and an OpenClaw alternatives blog. | Needs an open source repo. |
| **AlternativeTo** | Add bleep, then add it as an alternative on the Grok Bot, OpenClaw (clawdbot) and OpenMausBot pages. Free, no OSS rule, ranks well in Google and is cited by LLMs. | Can do now, before open sourcing. |
| **Product Hunt** | Launch day. Cited by LLMs, gives a high-authority link. | Prepare the GIF, maker comment, first 10 supporters. Tuesday to Thursday. |
| **Hacker News "Show HN"** | Developer audience that uses Claude Code. | Title: "Show HN: bleep, a Mac buddy that approves safe Claude Code asks and watches your inbox". Post when the repo is public. |
| **awesome lists** | Durable GitHub links LLMs train on. | PRs to awesome-claude-code, awesome-mcp-clients, awesome-macos / open-source-mac-os-apps, awesome-tauri. |
| **Others** | toolradar, There's An AI For That, Futurepedia, MacUpdate, Best of JS (lists JS/TS projects; OpenMausBot is there), Uneed, Peerlist. | One afternoon. Use the same description each time. |
| **Homebrew core cask** | `brew install --cask bleep` without your tap is a quality signal. | Needs notability: roughly 75+ stars, 30+ forks or watchers. Do it after the launch. |

### Phase 2: Earned mentions (weeks 2 to 6)

1. **Pitch the list writers who already rank for your terms.** They are the sources LLMs cite. Send each a short, specific email: what bleep is, why it fits their list (proactive, local, free), a GIF, and the honest limit (no cloud VMs).
   - bitdoze.com "Best open source Grok Bot alternatives" (also DE and ES versions)
   - scriptbyai.com "10 best open-source Grok Bot alternatives"
   - tooljunction.io, simular.ai "Grok Bot alternatives"
   - opensourcealternatives.to Grok Bot guide
   - buildbetter.ai and vellum.ai "OpenClaw alternatives" posts; rywalker.com
   - feedbagel.com, chatgate.ai (they covered CC-Beeper)
2. **Mac press.** MacStories (wrote the OpenClaw story), iGeeksBlog (covered Grok Bot for Mac), 9to5Mac tips line, MacRumors forums. Angle: "a free, local, proactive Mac assistant in the year of Grok Bot".
3. **Reddit, done properly.** Research found Reddit presence among the strongest predictors of ChatGPT recommendations. r/macapps (expects a dev post with honest details), r/ClaudeAI, r/ClaudeCode, r/SideProject. Be a real participant: answer "is there a Grok Bot alternative" and "how do I stop babysitting Claude Code" threads when bleep fits, and say you made it. No fake accounts or vote asking.
4. **YouTube.** You already have `scripts/video/`. Publish a 60 to 90 second demo and a 5 minute "bleep vs Grok Bot" walkthrough. Google AI Overviews cite YouTube often, and the transcript is indexed.
5. **Build posts.** One dev.to or Hashnode post: "How I built a floating Mac assistant with Tauri, Rust and Claude Code". One on your own site. Link both to the domain. These rank for technical terms and get you into training data.
6. **X / LinkedIn.** Short demo clips. Tag the Claude Code community, reply in Grok Bot launch threads with a fair comparison.

### Phase 3: Measure (monthly)

- **Prompt panel.** Each month, ask ChatGPT, Perplexity, Gemini, Claude and Google AI Mode the same 10 prompts and log if bleep appears and which source is cited. Example prompts: "best open source Grok Bot alternative", "OpenClaw alternative for Mac", "app that approves Claude Code permissions automatically", "free AI assistant for Mac that reads Gmail and Calendar". The cited sources tell you who to pitch next.
- **Search Console and Bing.** Impressions for the term clusters in section 2.3.
- **Vercel Analytics referrers.** Watch `chatgpt.com`, `perplexity.ai`, `gemini.google.com`, `claude.ai`. Add `?ref=` on directory links you control.
- **GitHub stars and Homebrew installs.** Stars are the ranking signal in every OSS directory.

---

## 6. Things to avoid

- **Claiming feature parity with Grok Bot.** It is not true and reviewers will say so. Claim the different strengths.
- **Using xAI, OpenClaw or other logos, or implying a link.** Name them in text for comparison only. Add "Not affiliated with xAI, Anthropic or OpenAI" in the footer.
- **Fake reviews, sock puppet Reddit posts, bought listicle spots without disclosure.** Google's AI guide warns against inauthentic mentions, and Reddit bans fast.
- **Calling it "open source" before the repo is public and licensed.**
- **The bare name "bleep".** Four other Mac apps use it, and "bleep" means a censor sound. Always write "bleep for Mac", "bleep AI assistant" or "bleep buddy" in titles, alt text, repo names and posts.

---

## 7. Decisions needed from you

| ID | Decision | Recommendation |
|---|---|---|
| **D1** | Open source the app repo? | **Yes**, if you want opensourcealternatives.to, openalternative.co and the "open source Grok Bot alternative" lists. Without it, those channels are closed and the lists will skip bleep. License: MIT for reach, or AGPL-3.0 / FSL if you want to block a hosted clone. Before you flip it public: scan history for secrets (the Google client ID and secret are compiled in from env, so check no `.env` or log was ever committed), and decide what to do with `mocks/`, `brand/`, `undefined/`. |
| **D2** | Domain | Buy one before any launch or outreach so every link lands on it. Options with no DNS on 18 Nov: `usebleep.app`, `trybleep.app`, `bleepbuddy.app`. `bleep.app` is for sale on Afternic if the price is fair; a short brand domain helps name collisions most. |
| **D3** | Lead with "Grok Bot alternative" or "Claude Code companion"? | Do both, in that order of effort: home page leads with "AI assistant for Mac", the Claude Code page owns the easy cluster, `/vs/grok-bot` catches the trend. |

---

## 8. Execution order

1. D1, D2.
2. Code (one PR): domain switch, `robots.txt`, `sitemap.xml`, `vercel.json`, title, H1, summary paragraph, subheads, JSON-LD, compare block, version line, screenshots, footer disclaimer, `llms.txt`.
3. Code (second PR): `/vs/grok-bot`, `/vs/openclaw`, `/vs/openmausbot`, `/alternatives`, `/claude-code-companion`, `/changelog`, `/privacy`.
4. README, topics, public repo, release.
5. Search Console, Bing, IndexNow.
6. AlternativeTo now. The other directories on the day the repo goes public.
7. Show HN and Product Hunt in the same week. Reddit posts after.
8. Listicle and press outreach.
9. Monthly prompt panel.

---

## Sources

- Grok Bot: [x.ai announcement](https://x.ai/news/introducing-grok-bot), [VentureBeat](https://venturebeat.com/orchestration/spacexais-grok-bot-turns-agents-into-persistent-digital-coworkers-that-can-operate-your-apps-for-120-per-month), [eesel](https://www.eesel.ai/blog/grok-bot), [iGeeksBlog](https://www.igeeksblog.com/grok-bot-macos-ios-launch/)
- OpenMausBot: [GitHub](https://github.com/milind-soni/OpenMausBot), [site](https://openmausbot.vercel.app/), [Best of JS](https://bestofjs.org/projects/openmausbot)
- Alternatives lists: [opensourcealternatives.to Grok Bot](https://www.opensourcealternatives.to/alternative-to/grok-bot), [bitdoze](https://www.bitdoze.com/best-open-source-grok-bot-alternatives/), [scriptbyai](https://www.scriptbyai.com/best-open-source-grok-bot-alternatives/), [openalternative Grok Bot](https://openalternative.co/alternatives/grok-bot), [openalternative OpenClaw](https://openalternative.co/blog/best-openclaw-alternatives), [AlternativeTo Grok Bot](https://alternativeto.net/software/grok-bot/), [buildbetter OpenClaw alternatives](https://blog.buildbetter.ai/best-openclaw-alternatives-in-2026-local-first-personal-ai-agents-compared/), [OpenClaw on Wikipedia](https://en.wikipedia.org/wiki/OpenClaw), [MacStories](https://www.macstories.net/stories/clawdbot-showed-me-what-the-future-of-personal-ai-assistants-looks-like/)
- Directory rules: [opensourcealternatives.to submit](https://www.opensourcealternatives.to/submit), [CC-Beeper listing](https://www.opensourcealternatives.to/item/cc-beeper), [openalternative.co](https://openalternative.co/submit)
- Claude Code companions: [CC-Beeper on Product Hunt](https://www.producthunt.com/products/cc-beeper), [Buddi](https://github.com/talkvalue/Buddi)
- GEO evidence: [Ahrefs backlink and GEO playbook summary](https://aiidelist.com/blog/how-to-get-chatgpt-to-recommend-your-saas), [Hackernoon discovery gap study](https://sia.hackernoon.com/the-discovery-gap-why-chatgpt-knows-your-startup-but-wont-recommend-it), [gogochimp ChatGPT SEO](https://www.gogochimp.com/blog/chatgpt-seo), [Alhena on reviews](https://alhena.ai/blog/geo-ecommerce-product-brands/)
- llms.txt: [ppc.land on Ahrefs data](https://ppc.land/llms-txt-adoption-rises-8-8x-but-97-of-files-get-zero-ai-requests/), [SEOmator census](https://seomator.com/blog/llms-txt-adoption), [Optimizely](https://www.optimizely.com/field-notes/articles/does-llms-txt-work/)
- Google AI guide: [Search Engine Land](https://searchengineland.com/google-ai-search-experiences-success-455845), [Previsible](https://previsible.io/seo-ai-news/google-published-its-first-guide-to-ai-search-optimization/), [Practical Ecommerce](https://www.practicalecommerce.com/googles-optimization-guidelines-for-ai-mode)
- Name collisions: [Bleep visual notes](https://apps.apple.com/app/bleep-think-visually/id6478577586), [BitTorrent Bleep](https://9to5mac.com/2015/05/13/bittorrent-releases-bleep-a-secure-peer-to-peer-messaging-service-for-ios-and-mac-video/)

Caveats: many GEO numbers come from vendor blogs and small samples. Weight the Princeton study and the large Ahrefs datasets highest. Star counts and listings were read on 18 Nov 2026 and change fast.
