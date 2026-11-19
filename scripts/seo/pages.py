#!/usr/bin/env python3
"""Build the landing site's text pages, sitemap.xml and changelog.

Run from the repo root after a release or a copy change:

    python3 scripts/seo/pages.py

It writes landing/vs/*.html, landing/alternatives.html, landing/claude-code-companion.html,
landing/privacy.html, landing/changelog.html and landing/sitemap.xml. The changelog comes from
the GitHub releases of AviroopPaul/bleep (needs `gh`). Edit the copy here, not in the HTML.
No em dashes in any copy.
"""
import datetime
import html
import json
import os
import re
import subprocess

ROOT = os.path.join(os.path.dirname(__file__), "..", "..", "landing")
SITE = "https://bleep-omega.vercel.app"
UPDATED = "2026-11-19"
UPDATED_TEXT = "19 Nov 2026"
VERSION = "0.5.1"
BREW = "brew install --cask AviroopPaul/tap/bleep"
CURL = f"curl -fsSL {SITE}/install.sh | sh"

AUTHOR = {"@type": "Person", "@id": SITE + "/#author", "name": "Aviroop Paul", "url": "https://avirooppaul.site",
          "sameAs": ["https://github.com/AviroopPaul", "https://linkedin.com/in/avirooppaul"]}
APP_REF = {"@type": "SoftwareApplication", "@id": SITE + "/#app", "name": "bleep", "url": SITE + "/",
           "applicationCategory": "ProductivityApplication", "operatingSystem": "macOS 15 or later (Apple silicon)",
           "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"}}

FOOTER_LINKS = [
    ("https://github.com/AviroopPaul/bleep", "Source on GitHub"),
    ("https://github.com/AviroopPaul/bleep/releases", "Releases"),
    ("/changelog", "Changelog"),
    ("/alternatives", "Alternatives"),
    ("/vs/grok-bot", "vs Grok Bot"),
    ("/vs/openclaw", "vs OpenClaw"),
    ("/vs/openmausbot", "vs OpenMausBot"),
    ("/claude-code-companion", "For Claude Code"),
    ("/privacy", "Privacy"),
]


def strip(s):
    return html.unescape(re.sub(r"<[^>]+>", "", s)).strip()


def table(head, rows):
    """head: list of column titles (first is blank). Column 1 is bleep and is highlighted."""
    th = "".join(f'<th scope="col"{" class=\"us\"" if i == 1 else ""}>{h}</th>' for i, h in enumerate(head))
    body = ""
    for r in rows:
        cells = "".join(f'<td{" class=\"us\"" if i == 0 else ""}>{c}</td>' for i, c in enumerate(r[1:]))
        body += f'<tr><th scope="row">{r[0]}</th>{cells}</tr>\n'
    return f'<div class="tbl"><table><thead><tr>{th}</tr></thead><tbody>\n{body}</tbody></table></div>'


def faq_html(items):
    out = "".join(f"<details><summary>{q}</summary><p>{a}</p></details>\n" for q, a in items)
    return f'<h2 id="faq">Questions</h2>\n<div class="faq">\n{out}</div>'


def page(path, title, desc, crumbs, h1, lede, body, faq=None, kind="Article", sources=None, cta=True):
    url = SITE + ("/" + path if path else "/")
    graph = [
        {"@type": kind, "@id": url + "#page", "headline": h1 if kind == "Article" else None, "name": title,
         "description": desc, "url": url, "datePublished": UPDATED, "dateModified": UPDATED,
         "author": {"@id": AUTHOR["@id"]}, "publisher": {"@id": AUTHOR["@id"]}, "about": {"@id": APP_REF["@id"]},
         "image": SITE + "/og.png", "inLanguage": "en"},
        {"@type": "BreadcrumbList", "itemListElement": [
            {"@type": "ListItem", "position": i + 1, "name": n, "item": SITE + u}
            for i, (u, n) in enumerate([("/", "bleep")] + crumbs)]},
        AUTHOR, APP_REF,
    ]
    graph[0] = {k: v for k, v in graph[0].items() if v is not None}
    if faq:
        graph.append({"@type": "FAQPage", "mainEntity": [
            {"@type": "Question", "name": strip(q), "acceptedAnswer": {"@type": "Answer", "text": strip(a)}} for q, a in faq]})
    ld = json.dumps({"@context": "https://schema.org", "@graph": graph}, ensure_ascii=False, indent=1).replace("</", "<\\/")
    crumb_html = " / ".join([f'<a href="/">bleep</a>'] + [f'<a href="{u}">{n}</a>' if i < len(crumbs) - 1 else n
                                                          for i, (u, n) in enumerate(crumbs)])
    src_html = ""
    if sources:
        src_html = '<h2>Sources</h2>\n<ul class="src">' + "".join(
            f'<li><a href="{u}" rel="nofollow noopener">{n}</a></li>' for n, u in sources) + "</ul>"
    cta_html = ""
    if cta:
        cta_html = f'''<section class="cta">
  <h2>Try bleep on your Mac</h2>
  <p>Free and open source. macOS 15+ on Apple silicon.</p>
  <div class="cmd"><code>{BREW}</code><button type="button" data-copy="{BREW}">Copy</button></div>
  <p class="alt">No Homebrew? <code>{CURL}</code> · <a href="https://github.com/AviroopPaul/bleep">Source on GitHub</a></p>
</section>'''
    foot = "\n      ".join(f'<a href="{u}">{n}</a>' for u, n in FOOTER_LINKS)
    return f'''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)}</title>
<meta name="description" content="{html.escape(desc)}">
<link rel="canonical" href="{url}">
<meta name="robots" content="index, follow, max-image-preview:large">
<meta name="author" content="Aviroop Paul">
<meta property="og:type" content="article">
<meta property="og:site_name" content="bleep">
<meta property="og:url" content="{url}">
<meta property="og:title" content="{html.escape(title)}">
<meta property="og:description" content="{html.escape(desc)}">
<meta property="og:image" content="{SITE}/og.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{html.escape(title)}">
<meta name="twitter:description" content="{html.escape(desc)}">
<meta name="twitter:image" content="{SITE}/og.png">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="icon" href="/favicon-32.png" type="image/png" sizes="32x32">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<meta name="theme-color" content="#FDFDFD">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600&family=Geist+Mono:wght@500&display=swap" rel="stylesheet">
<link rel="stylesheet" href="/pages.css">
<script type="application/ld+json">
{ld}
</script>
<script>window.va = window.va || function () {{ (window.vaq = window.vaq || []).push(arguments); }};</script>
<script defer src="/_vercel/insights/script.js"></script>
</head>
<body>
<nav class="top"><div class="bar">
  <a class="brand" href="/"><img src="/apple-touch-icon.png" alt="" width="30" height="30">bleep</a>
  <div class="links"><a href="/#cases">Features</a><a href="/alternatives">Compare</a><a href="/claude-code-companion">Claude Code</a><a href="/#faq">FAQ</a></div>
  <a class="npill" href="https://github.com/AviroopPaul/bleep">GitHub</a>
</div></nav>
<main class="wrap">
<p class="crumbs">{crumb_html}</p>
<h1>{h1}</h1>
<p class="lede">{lede}</p>
<p class="meta">Updated {UPDATED_TEXT} · by <a href="https://avirooppaul.site">Aviroop Paul</a>, who makes bleep · bleep {VERSION}</p>
{body}
{faq_html(faq) if faq else ""}
{src_html}
{cta_html}
<footer>
  <nav aria-label="More about bleep">
      {foot}
  </nav>
  <p>bleep is a free, open source project by <a href="https://avirooppaul.site">Aviroop Paul</a>, MIT license. Not affiliated with Anthropic, OpenAI, Google or xAI. Product names belong to their owners.</p>
</footer>
</main>
<script>
document.querySelectorAll('[data-copy]').forEach(b => b.addEventListener('click', () => {{
  navigator.clipboard && navigator.clipboard.writeText(b.dataset.copy).then(() => {{ b.textContent = 'Copied'; setTimeout(() => b.textContent = 'Copy', 2000); }});
}}));
</script>
</body>
</html>
'''


def write(path, content):
    full = os.path.join(ROOT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    assert "—" not in content, f"em dash in {path}"
    with open(full, "w") as f:
        f.write(content)
    print("wrote", path)


# ---------------------------------------------------------------- shared facts

BLEEP_FACTS = """<ul>
<li><b>Lives on your Mac.</b> A native app with a small Rust core. A floating buddy on every Space. Double-tap <code>⌥</code> to call it.</li>
<li><b>Speaks first.</b> It checks your mail, calendar and coding agents on its own, and taps you before a meeting, when a bill is due, when a mail waits on you, or when an agent is stuck.</li>
<li><b>Unblocks your coding agents.</b> It approves the safe Claude Code and Codex asks your rules allow, like tests, lint and file reads. Push, deploy, delete, secrets and money always wait for you.</li>
<li><b>Any model.</b> Your Claude Code plan by default, an API key from Anthropic, OpenAI, Gemini, OpenRouter or Groq, or a local model in Ollama or LM Studio.</li>
<li><b>Private.</b> No bleep server, no account, no telemetry. Mail, memory and logs stay in <code>~/.bleep</code>.</li>
<li><b>Free and open source.</b> MIT license, source on <a href="https://github.com/AviroopPaul/bleep">GitHub</a>.</li>
</ul>"""

# ---------------------------------------------------------------- /vs/grok-bot

GROK_SOURCES = [
    ("xAI: Introducing Grok Bot", "https://x.ai/news/introducing-grok-bot"),
    ("VentureBeat on Grok Bot", "https://venturebeat.com/orchestration/spacexais-grok-bot-turns-agents-into-persistent-digital-coworkers-that-can-operate-your-apps-for-120-per-month"),
    ("eesel: Grok Bot explained", "https://www.eesel.ai/blog/grok-bot"),
    ("iGeeksBlog: Grok Bot on Mac and iPhone", "https://www.igeeksblog.com/grok-bot-macos-ios-launch/"),
]

grok_body = f"""
<section class="tldr"><h2>Short answer</h2><ul>
<li><b>Pick Grok Bot</b> if you want cloud agents that do long, multi-step work inside other apps and websites for you, and you already pay for SuperGrok Heavy or Cursor Ultra.</li>
<li><b>Pick bleep</b> if you want a free assistant on your Mac that watches your mail, calendar and coding agents, speaks first when something needs you, and keeps your data on your machine.</li>
</ul></section>

<h2>bleep vs Grok Bot at a glance</h2>
{table(["", "bleep", "Grok Bot"], [
    ["What it is", "A proactive AI buddy that floats on your Mac", "Persistent AI teammates from xAI, each with its own cloud computer"],
    ["Where it runs", "On your Mac. No bleep server.", "On managed Linux VMs in xAI's cloud"],
    ["How it helps", "Watches mail, calendar and coding agents, taps you, does small jobs after your yes", "Signs in to your apps and websites and works through multi-step tasks"],
    ["Speaks first", "Yes, on its own: meetings, bills, waiting mail, stuck agents", "Runs tasks and routines you set up"],
    ["Coding agents", "Approves safe Claude Code and Codex asks by rules, logs every one", "Not its focus"],
    ["Model", "Claude Code plan, any API key, or local models", "Grok"],
    ["Access", "Download now", "Beta for SuperGrok Heavy, Cursor Ultra and Cursor Teams Premium. Enterprise waitlist."],
    ["Price", "Free", "Needs a paid plan"],
    ["Platforms", "macOS 15+ on Apple silicon", "Desktop and iOS"],
    ["Open source", "Yes, MIT", "No"],
])}

<h2>What Grok Bot does better</h2>
<ul>
<li><b>It does the work itself.</b> Each bot has its own computer with a browser and a terminal. It can operate apps and websites that have no API.</li>
<li><b>Many bots at once.</b> You can run several bots in parallel, each with its own memory, like a small team.</li>
<li><b>Routines from demos.</b> Show a bot how you do a task once and it can repeat it.</li>
<li><b>On your phone.</b> It has an iOS app. bleep is Mac only.</li>
</ul>

<h2>What bleep does better</h2>
<ul>
<li><b>It is free and you can start today.</b> No paid plan, no waitlist.</li>
<li><b>Your data stays on your Mac.</b> bleep has no server. Grok Bot signs in to your accounts from xAI's cloud.</li>
<li><b>It speaks first.</b> bleep is built to notice things: a meeting in an hour, a bill due tonight, a reply that waits on you.</li>
<li><b>It keeps your coding agents moving.</b> bleep sees every Claude Code and Codex session that waits on you and approves the safe asks. Rules decide, not a model, so a bad guess cannot approve a <code>git push --force</code>.</li>
<li><b>You pick the model.</b> Use the Claude plan you already pay for, any API key, or a local model with Ollama.</li>
<li><b>Open source.</b> Read the code, change it, or audit what it does with your mail.</li>
</ul>

<h2>Can you use both?</h2>
<p>Yes. They do different jobs. Grok Bot is a worker you hand tasks to. bleep is a buddy that watches your day and tells you what needs you. Many people want the second one first: fewer missed meetings and fewer agents stuck on a yes.</p>

<h2>What bleep is</h2>
{BLEEP_FACTS}
"""

grok_faq = [
    ("Is there a free Grok Bot alternative for Mac?", "Yes. bleep is free and open source, and runs on your Mac. It is not a clone: it does not run cloud computers or click through apps. It watches your mail, calendar and coding agents, speaks first when something needs you, and does small jobs after your yes."),
    ("Is there an open source Grok Bot alternative?", "Several. OpenMausBot and rakazo copy Grok Bot's team-of-bots design. bleep is open source too (MIT), but it is a single proactive assistant on your Mac rather than a bot team. See <a href=\"/alternatives\">all alternatives</a>."),
    ("Does bleep need a Grok or xAI account?", "No. bleep uses your Claude Code plan by default. You can also use an API key from Anthropic, OpenAI, Google Gemini, OpenRouter or Groq, or a local model."),
    ("Can bleep operate apps for me like Grok Bot?", "Not in the same way. bleep works through connectors: Gmail, Google Calendar, Google Drive, files on your Mac, and any remote MCP server. It does not control a browser or a virtual machine."),
]

# ---------------------------------------------------------------- /vs/openclaw

CLAW_SOURCES = [
    ("OpenClaw on Wikipedia", "https://en.wikipedia.org/wiki/OpenClaw"),
    ("MacStories on OpenClaw", "https://www.macstories.net/stories/clawdbot-showed-me-what-the-future-of-personal-ai-assistants-looks-like/"),
    ("opensourcealternatives.to: Grok Bot alternatives", "https://www.opensourcealternatives.to/alternative-to/grok-bot"),
]

claw_body = f"""
<section class="tldr"><h2>Short answer</h2><ul>
<li><b>Pick OpenClaw</b> if you want a general assistant you message from your chat apps, with a large plugin ecosystem, on a machine or server you run.</li>
<li><b>Pick bleep</b> if you want a native Mac buddy that watches your mail, calendar and Claude Code or Codex sessions on its own, with no setup beyond one install command.</li>
</ul></section>

<h2>bleep vs OpenClaw at a glance</h2>
{table(["", "bleep", "OpenClaw"], [
    ["What it is", "A proactive AI buddy that floats on your Mac", "An open source personal AI assistant you talk to from chat apps"],
    ["Where you talk to it", "A floating chat on your desktop, or your voice", "Messaging apps you already use"],
    ["Where it runs", "Native Mac app", "Your machine or a server you run"],
    ["Speaks first", "Yes, on its own", "On schedules you set up"],
    ["Coding agents", "Approves safe Claude Code and Codex asks by rules", "Not its focus"],
    ["Setup", "One command, then a short guided setup", "Install and configure on a machine you keep running"],
    ["Model", "Claude Code plan, any API key, or local models", "Your choice of model"],
    ["Price", "Free", "Free, you pay for the model"],
    ["Open source", "Yes, MIT", "Yes"],
])}

<h2>What OpenClaw does better</h2>
<ul>
<li><b>Reach.</b> You can talk to it from your phone through chat apps, wherever you are.</li>
<li><b>Ecosystem.</b> It is one of the most starred projects on GitHub, with many skills and plugins from its community.</li>
<li><b>Cross platform.</b> It is not tied to macOS.</li>
</ul>

<h2>What bleep does better</h2>
<ul>
<li><b>It is a Mac app, not a server.</b> Install in one line, and it lives on your desktop as a small buddy.</li>
<li><b>Proactive out of the box.</b> Meeting prep, bill reminders and waiting mail work after setup, with quiet hours and a ping limit.</li>
<li><b>Built for coding agents.</b> bleep watches every Claude Code and Codex session and approves the safe asks by fixed rules, with a log.</li>
<li><b>Careful by design.</b> Sending mail, moving events, deleting and running commands always show a card and wait for your yes. Text in your mail is treated as data, not as instructions.</li>
</ul>

<h2>What bleep is</h2>
{BLEEP_FACTS}
"""

claw_faq = [
    ("Is bleep an OpenClaw alternative for Mac?", "Yes, for people who want a native Mac assistant rather than a chat bot on a server. bleep floats on your desktop, watches your mail, calendar and coding agents, and taps you when something needs you."),
    ("Does bleep work with WhatsApp or Telegram?", "No. You talk to bleep in its own chat on your Mac, or by voice with the optional wake word."),
    ("Can bleep use the same models as OpenClaw?", "Mostly. bleep runs on your Claude Code plan by default, or on an API key from Anthropic, OpenAI, Gemini, OpenRouter or Groq, or on a local model in Ollama or LM Studio."),
]

# ---------------------------------------------------------------- /vs/openmausbot

MAUS_SOURCES = [
    ("OpenMausBot on GitHub", "https://github.com/milind-soni/OpenMausBot"),
    ("OpenMausBot website", "https://openmausbot.vercel.app/"),
    ("opensourcealternatives.to: Grok Bot alternatives", "https://www.opensourcealternatives.to/alternative-to/grok-bot"),
]

maus_body = f"""
<section class="tldr"><h2>Short answer</h2><ul>
<li><b>Pick OpenMausBot</b> if you want an open source version of Grok Bot: a team of agent bots, each with its own machine, run on the Claude, Codex or Grok CLIs you already pay for.</li>
<li><b>Pick bleep</b> if you want one buddy on your Mac that watches your day, speaks first, and keeps your coding agents moving.</li>
</ul></section>

<h2>bleep vs OpenMausBot at a glance</h2>
{table(["", "bleep", "OpenMausBot"], [
    ["What it is", "A proactive AI buddy that floats on your Mac", "An open source Grok Bot alternative: a team of AI bots in a chat app"],
    ["How it helps", "Watches mail, calendar and coding agents, taps you, does small jobs after your yes", "Bots take tasks, each with its own model, apps and cloud desktop or local VM"],
    ["Speaks first", "Yes, on its own", "When you message it"],
    ["Coding agents", "Approves safe Claude Code and Codex asks in your own sessions", "Bots are coding agents themselves"],
    ["Model", "Claude Code plan, any API key, or local models", "Claude, Codex or Grok Build CLIs"],
    ["Mail and calendar", "Gmail and Google Calendar built in", "Through the apps you give each bot"],
    ["Price", "Free", "Free"],
    ["Open source", "Yes, MIT", "Yes"],
])}

<h2>How they differ</h2>
<p>Both use the AI subscriptions you already have, so neither adds a bill. The difference is the job. OpenMausBot gives you workers: you hand a bot a task and it works on its own machine. bleep gives you a watcher: it sits on your desktop, notices what needs you, and handles the small asks so your own agents and your own day keep moving.</p>

<h2>What bleep is</h2>
{BLEEP_FACTS}
"""

maus_faq = [
    ("Is bleep a fork of OpenMausBot?", "No. bleep is a separate project with its own Rust core, built with Tauri 2."),
    ("Can I use bleep and OpenMausBot together?", "Yes. bleep can watch the Claude Code and Codex sessions on your Mac and approve their safe asks."),
]

# ---------------------------------------------------------------- /alternatives

alt_body = """
<section class="tldr"><h2>Short answer</h2><ul>
<li><b>Want cloud agents that do the work?</b> Grok Bot (paid) or its open source copies, OpenMausBot and rakazo.</li>
<li><b>Want an assistant in your chat apps?</b> OpenClaw.</li>
<li><b>Want a free buddy on your Mac that watches your day and your coding agents?</b> bleep.</li>
<li><b>Only want Claude Code alerts?</b> CC-Beeper or Buddi.</li>
</ul></section>

<h2>Personal AI assistants</h2>

<div class="tool"><h3><a href="/">bleep</a></h3><p class="tags">Mac · free · open source (MIT) · proactive</p>
<p>A small AI buddy that floats on your Mac. It watches Claude Code, Codex, Gmail and Google Calendar, approves the safe agent asks, preps your meetings, catches bills, and taps you only when it matters. Runs on your Claude Code plan, any API key, or a local model.</p></div>

<div class="tool"><h3>Grok Bot</h3><p class="tags">Cloud · paid plan · closed source</p>
<p>xAI's persistent AI teammates. Each bot has its own cloud computer, signs in to your apps, and works on multi-step tasks. In beta for SuperGrok Heavy and Cursor Ultra subscribers. <a href="/vs/grok-bot">bleep vs Grok Bot</a>.</p></div>

<div class="tool"><h3>OpenClaw</h3><p class="tags">Self-hosted · free · open source</p>
<p>A popular open source personal assistant you message from your chat apps, with a large plugin ecosystem. <a href="/vs/openclaw">bleep vs OpenClaw</a>.</p></div>

<div class="tool"><h3>OpenMausBot</h3><p class="tags">Desktop plus VMs · free · open source</p>
<p>An open source Grok Bot alternative. A team of bots, each a Claude, Codex or Grok Build agent with its own machine, on the CLI plans you already pay for. <a href="/vs/openmausbot">bleep vs OpenMausBot</a>.</p></div>

<div class="tool"><h3>OpenBot</h3><p class="tags">Self-hosted · free · open source</p>
<p>Agent "coworkers", each in its own container with a browser, with a policy-gated audit trail. Aimed at teams.</p></div>

<div class="tool"><h3>rakazo</h3><p class="tags">Self-hosted · free · open source (Apache-2.0)</p>
<p>Often named the closest open source match to Grok Bot's design: persistent named bots with their own memory, browser and terminal.</p></div>

<div class="tool"><h3>Hermes Agent</h3><p class="tags">Self-hosted · free · open source (MIT)</p>
<p>A power-user agent you run yourself, with many compute backends.</p></div>

<h2>Claude Code companions for Mac</h2>
<p>Smaller apps that sit next to Claude Code. They tell you when a session needs you. bleep also approves the safe asks for you, and covers your mail and calendar.</p>

<div class="tool"><h3><a href="/claude-code-companion">bleep</a></h3><p class="tags">Approves safe asks by rules · Codex too · mail and calendar</p>
<p>Watches every Claude Code and Codex session, approves tests, lint and reads, and holds push, deploy and delete for you.</p></div>

<div class="tool"><h3>CC-Beeper</h3><p class="tags">Free · open source (GPL-3.0)</p>
<p>A floating macOS pager for Claude Code with status, alerts and voice replies.</p></div>

<div class="tool"><h3>Buddi</h3><p class="tags">Free · open source</p>
<p>A notch companion for Claude Code. Approve or deny tool calls from the notch.</p></div>

<h2>How to choose</h2>
<ul>
<li><b>Where should your data live?</b> On your Mac (bleep, CC-Beeper, Buddi), on a machine you run (OpenClaw, OpenBot, rakazo), or in a vendor cloud (Grok Bot).</li>
<li><b>Do you want a worker or a watcher?</b> Workers take tasks and run for a long time. A watcher notices what needs you and keeps the rest quiet.</li>
<li><b>What do you already pay for?</b> bleep and OpenMausBot reuse your Claude or Codex plan. Grok Bot needs its own plan.</li>
</ul>
<p>Facts about other tools come from their own pages and public reviews as of November 2026. Something out of date? <a href="https://github.com/AviroopPaul/bleep/issues">Open an issue</a>.</p>
"""

alt_faq = [
    ("What is the best free alternative to Grok Bot for Mac?", "It depends on the job. For a free assistant that runs on your Mac and speaks first about your mail, calendar and coding agents, try bleep. For a free team of agent bots like Grok Bot, try OpenMausBot or rakazo."),
    ("Which personal AI assistants are open source?", "bleep (MIT), OpenClaw, OpenMausBot, OpenBot, rakazo (Apache-2.0) and Hermes Agent (MIT) are all open source. Grok Bot is closed source."),
    ("Which AI assistant keeps my data on my Mac?", "bleep has no server and keeps mail, memory and logs in ~/.bleep on your Mac. With a local model in Ollama or LM Studio, nothing leaves your Mac at all."),
]

ALT_SOURCES = [
    ("opensourcealternatives.to: Grok Bot alternatives", "https://www.opensourcealternatives.to/alternative-to/grok-bot"),
    ("openalternative.co: Grok Bot alternatives", "https://openalternative.co/alternatives/grok-bot"),
    ("OpenClaw on Wikipedia", "https://en.wikipedia.org/wiki/OpenClaw"),
    ("OpenMausBot on GitHub", "https://github.com/milind-soni/OpenMausBot"),
    ("CC-Beeper on opensourcealternatives.to", "https://www.opensourcealternatives.to/item/cc-beeper"),
    ("Buddi on GitHub", "https://github.com/talkvalue/Buddi"),
]

# ---------------------------------------------------------------- /claude-code-companion

cc_body = f"""
<section class="tldr"><h2>Short answer</h2><ul>
<li>bleep installs hooks in Claude Code and turn notices in Codex, so it sees every session that waits on you.</li>
<li>Rules in code, not a model, decide what passes. Tests, lint and reads pass. Push, deploy, delete, secrets and money always wait for you.</li>
<li>Every approval is logged. You see the rest in one Agent sessions window with a short summary of each ask.</li>
</ul></section>

<h2>The problem</h2>
<p>You start three Claude Code sessions and walk away. Ten minutes later, two of them sit idle, waiting for you to type yes to <code>npm test</code>. The third wants to <code>git push --force</code>, and that one really should wait for you. bleep tells these apart.</p>

<h2>How it works</h2>
<ol>
<li><b>Hooks.</b> In setup, bleep adds a small hook to Claude Code and a notify command to Codex. You can remove them in Settings, Agents.</li>
<li><b>Rules.</b> Each ask is checked against <code>~/.bleep/agent-rules.toml</code>. A command joined with <code>&amp;&amp;</code>, <code>;</code> or <code>|</code> passes only if every part matches and nothing risky appears anywhere in it. Shell globs always ask.</li>
<li><b>Screenshots.</b> When an agent asks you to look at a screenshot, bleep can check it against the ask and let it continue if it matches.</li>
<li><b>You.</b> Anything the rules cannot read safely comes to you, with a short summary, in the chat and the Agent sessions window.</li>
</ol>

<h2>Default rules</h2>
{table(["", "bleep approves", "Always waits for you"], [
    ["Commands", "<code>npm test</code>, <code>npx vitest run</code>, <code>npx playwright test</code>, <code>cargo test</code>, <code>cargo clippy</code>, <code>pytest</code>, <code>go test</code>, <code>ruff check</code>, lint and type checks", "<code>git push</code>, <code>--force</code>, <code>rm</code>, <code>sudo</code>, deploy commands, package installs, <code>curl</code>, <code>ssh</code>, <code>git reset</code>, <code>git rebase</code>"],
    ["Reading", "<code>Read</code>, <code>Glob</code>, <code>Grep</code>, <code>git status</code>, <code>git diff</code>, <code>git log</code>, <code>ls</code>, <code>cat</code>", "Anything that touches <code>.env</code>, keys, tokens, <code>.ssh</code>, <code>.aws</code>, credentials or passwords"],
    ["Other", "A screenshot that matches the ask", "Money, sending mail, anything outside the sandbox"],
])}
<p>You can edit the lists. A model never turns an ask into a yes.</p>

<h2>Compared with other Claude Code companions</h2>
{table(["", "bleep", "CC-Beeper", "Buddi"], [
    ["Alerts when a session needs you", "Yes", "Yes", "Yes"],
    ["Approves safe asks for you", "Yes, by rules, with a log", "No", "You tap approve or deny"],
    ["Codex sessions", "Yes", "No", "No"],
    ["Mail, calendar, reminders", "Yes", "No", "No"],
    ["Open source", "MIT", "GPL-3.0", "Yes"],
])}

<h2>Also in bleep</h2>
{BLEEP_FACTS}
"""

cc_faq = [
    ("How do I auto-approve safe Claude Code permission prompts on Mac?", "Install bleep and let setup add its Claude Code hook. bleep then approves asks that match your rules (tests, lint, reads) and holds everything else for you. The rules live in ~/.bleep/agent-rules.toml."),
    ("Is this safer than Claude Code's own allow list?", "It adds checks. bleep looks at every part of a joined command, refuses shell globs, and keeps a deny list for secrets and risky commands that wins over any allow rule. It also logs every approval."),
    ("Does bleep use my Claude plan for approvals?", "No. Approvals are decided by rules in code, so they cost no model calls. Only the chat, check-ins and screenshot checks use the model."),
    ("Does it work with Codex?", "Yes. bleep installs a turn notice for Codex and shows those sessions next to Claude Code."),
]

# ---------------------------------------------------------------- /privacy

privacy_body = """
<h2>The short version</h2>
<ul>
<li><b>bleep has no server.</b> There is no bleep account and no telemetry in the app.</li>
<li><b>Your data stays on your Mac.</b> Mail summaries, memory, reminders, settings and logs are in <code>~/.bleep</code>. Google tokens and API keys are in the macOS Keychain.</li>
<li><b>The model gets only what it needs.</b> When bleep thinks, it sends the text it needs to the model you picked, through your own Claude Code login or API key. With a local model in Ollama or LM Studio, nothing leaves your Mac.</li>
</ul>

<h2>Google data</h2>
<p>If you connect Google, bleep asks for read access to Gmail, permission to create Gmail drafts, Google Calendar events and, if you turn it on, Google Drive. bleep reads this data on your Mac to answer you and to notice what needs you. It never sends mail, moves events or deletes anything without your tap on the exact action. It does not sell your data or use it for ads. You can disconnect Google in Settings at any time and remove access in your Google account.</p>

<h2>Voice</h2>
<p>The wake word is off by default. When it is on, speech to text runs on your Mac. Audio is never saved or sent anywhere.</p>

<h2>Updates</h2>
<p>bleep checks GitHub for a new release every few hours. That request carries no personal data. You can turn off automatic updates in Settings, Updates.</p>

<h2>This website</h2>
<p>This site uses Vercel Web Analytics, which counts page views without cookies. It has no ads and no third-party trackers.</p>

<h2>Remove everything</h2>
<p>Run <code>brew uninstall --cask --zap bleep</code>, or quit bleep, drag it to the Trash and delete <code>~/.bleep</code>. Remove the Claude Code and Codex hooks first in Settings, Agents.</p>

<h2>Contact</h2>
<p>Questions or a security report: <a href="https://github.com/AviroopPaul/bleep/security/advisories/new">report privately on GitHub</a> or open an <a href="https://github.com/AviroopPaul/bleep/issues">issue</a>.</p>
"""

# ---------------------------------------------------------------- /changelog


def md_inline(t):
    t = html.escape(t, quote=False)
    t = re.sub(r"\*\*(.+?)\*\*", r"<b>\1</b>", t)
    t = re.sub(r"`([^`]+)`", r"<code>\1</code>", t)
    t = re.sub(r"\[([^\]]+)\]\((https?://[^)]+)\)", r'<a href="\2">\1</a>', t)
    t = re.sub(r"(?<![\"'>])(https?://[^\s<]+)", r'<a href="\1">\1</a>', t)
    return t


def release_html(rel):
    body = rel["body"] or ""
    i = body.find("**New in")
    if i >= 0:
        body = body[i:]
    out, in_list, in_code = [], False, False
    for line in body.splitlines():
        if line.strip().startswith("```"):
            in_code = not in_code
            continue
        if in_code:
            continue
        if re.match(r"^\*\*New in [\d.]+\*\*\s*$", line.strip()):
            continue
        if line.startswith("- "):
            if not in_list:
                out.append("<ul>")
                in_list = True
            out.append(f"<li>{md_inline(line[2:])}</li>")
        else:
            if in_list:
                out.append("</ul>")
                in_list = False
            if line.strip():
                out.append(f"<p>{md_inline(line.strip())}</p>")
    if in_list:
        out.append("</ul>")
    d = datetime.date.fromisoformat(rel["date"]).strftime("%-d %b %Y")
    tag = rel["tag"]
    return (f'<section class="rel" id="{tag}"><h2>bleep {tag.lstrip("v")}</h2>'
            f'<p class="meta">{d} · <a href="https://github.com/AviroopPaul/bleep/releases/tag/{tag}">Release on GitHub</a></p>\n'
            + "\n".join(out) + "</section>")


def changelog_body():
    raw = subprocess.run(["gh", "api", "repos/AviroopPaul/bleep/releases?per_page=50",
                          "--jq", "[.[]|{tag:.tag_name,date:.published_at[0:10],body:.body}]"],
                         capture_output=True, text=True, check=True).stdout
    rels = json.loads(raw)
    return "\n".join(release_html(r) for r in rels)


# ---------------------------------------------------------------- build

PAGES = [
    ("vs/grok-bot", "bleep vs Grok Bot: A Free, Open Source Alternative for Mac",
     "Compare bleep and xAI's Grok Bot. bleep is a free, open source AI assistant on your Mac that watches your mail, calendar and coding agents. Grok Bot runs paid cloud agents.",
     [("/alternatives", "Alternatives"), ("/vs/grok-bot", "bleep vs Grok Bot")],
     "bleep vs Grok Bot",
     "Grok Bot gives you cloud teammates that do work in your apps. bleep is a free, open source buddy on your Mac that watches your day and your coding agents. Here is how they compare, and when to pick which.",
     grok_body, grok_faq, GROK_SOURCES),
    ("vs/openclaw", "bleep vs OpenClaw: A Native Mac AI Assistant",
     "Compare bleep and OpenClaw. Both are open source personal AI assistants. bleep is a native Mac app that speaks first about your mail, calendar and Claude Code sessions.",
     [("/alternatives", "Alternatives"), ("/vs/openclaw", "bleep vs OpenClaw")],
     "bleep vs OpenClaw",
     "OpenClaw is an assistant you message from chat apps. bleep is a native Mac app that floats on your desktop and taps you when something needs you. Both are free and open source.",
     claw_body, claw_faq, CLAW_SOURCES),
    ("vs/openmausbot", "bleep vs OpenMausBot: Two Open Source Grok Bot Alternatives",
     "Compare bleep and OpenMausBot. Both are free, open source and use the AI plans you already pay for. OpenMausBot runs a team of bots; bleep is one proactive buddy on your Mac.",
     [("/alternatives", "Alternatives"), ("/vs/openmausbot", "bleep vs OpenMausBot")],
     "bleep vs OpenMausBot",
     "Both are open source, both reuse the Claude or Codex plan you already have. OpenMausBot gives you a team of agent bots. bleep gives you one buddy that watches your day.",
     maus_body, maus_faq, MAUS_SOURCES),
    ("alternatives", "Grok Bot, OpenClaw and OpenMausBot Alternatives for Mac (2026)",
     "A fair list of personal AI assistants for Mac in 2026: bleep, Grok Bot, OpenClaw, OpenMausBot, OpenBot, rakazo, Hermes Agent, and Claude Code companions like CC-Beeper and Buddi.",
     [("/alternatives", "Alternatives")],
     "Personal AI assistants for Mac: Grok Bot alternatives and more",
     "Grok Bot made always-on AI assistants popular. Here are the main options in November 2026, what each one is for, and which ones are free and open source.",
     alt_body, alt_faq, ALT_SOURCES),
    ("claude-code-companion", "Claude Code Companion for Mac: Auto-Approve Safe Asks | bleep",
     "bleep watches every Claude Code and Codex session on your Mac. It approves safe asks like tests, lint and reads by fixed rules, and holds push, deploy and delete for you.",
     [("/claude-code-companion", "For Claude Code")],
     "A Claude Code companion that approves the safe asks",
     "bleep watches every Claude Code and Codex session on your Mac. Tests pass, lint passes, reads pass. A force push waits for you. Free and open source.",
     cc_body, cc_faq, None),
    ("privacy", "Privacy | bleep",
     "How bleep handles your data: no bleep server, no telemetry, data in ~/.bleep on your Mac, and only the text it needs goes to the model you pick.",
     [("/privacy", "Privacy")],
     "Privacy",
     "bleep is built to be trusted with your inbox. This page says what it reads, where it keeps it, and what leaves your Mac.",
     privacy_body, None, None),
]

if __name__ == "__main__":
    for path, title, desc, crumbs, h1, lede, body, faq, sources in PAGES:
        kind = "WebPage" if path == "privacy" else "Article"
        write(path + ".html", page(path, title, desc, crumbs, h1, lede, body, faq, kind, sources, cta=path != "privacy"))
    write("changelog.html", page("changelog", "Changelog | bleep, the AI assistant for Mac",
                                 "Every bleep release: new features and fixes in the free, open source AI assistant for Mac.",
                                 [("/changelog", "Changelog")], "Changelog",
                                 "What is new in each bleep release. bleep updates itself, so you get these on their own.",
                                 changelog_body(), None, "WebPage", None))
    urls = ["", "alternatives", "vs/grok-bot", "vs/openclaw", "vs/openmausbot", "claude-code-companion", "changelog", "privacy"]
    sm = "".join(f"  <url><loc>{SITE}/{u}</loc><lastmod>{UPDATED}</lastmod></url>\n" for u in urls)
    write("sitemap.xml", f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n{sm}</urlset>\n')
