//! What bleep may approve in my coding agents (build plan 8.11, D12). Rules in code
//! decide; a model never turns an ask into a yes. Anything the rules cannot read
//! safely goes to me.
use super::AskKind;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

/// One thing an agent wants to do.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub kind: AskKind,
    /// "Bash", "Read", "commandExecution", "fileChange", ...
    pub tool_name: String,
    pub command: Option<String>,
    pub paths: Vec<String>,
    /// The session's working folder: "the repo".
    pub cwd: String,
    /// Claude Code's `dangerouslyDisableSandbox`: the ask wants to leave the sandbox.
    pub sandbox_off: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AgentRules {
    pub auto: Vec<String>,
    pub ask: Vec<String>,
    pub read_tools: Vec<String>,
    /// Words that let bleep send the agent's own first button for me.
    pub answer_auto: Vec<String>,
    /// Words in a question or its first button that always keep it for me.
    pub answer_ask: Vec<String>,
}

pub const DEFAULT_RULES: &str = r#"# What bleep may approve in your coding agents. Anything not under auto asks you.
# "npm test" matches only that command. "cargo test *" also allows more arguments.
# A command joined with && ; | or a newline is approved only when every part matches,
# and nothing under ask appears anywhere in it. "~word" matches inside any word.
# Globs (* ? [ ] { }) outside quotes always ask: the shell expands them after bleep looks.
auto = ["npm test", "npm test *", "npm run test *", "npm run lint *", "npm run typecheck *", "npx vitest run *", "npx playwright test *",
  "npx tsc --noEmit *", "cargo test *", "cargo check *", "cargo clippy *", "cargo fmt --check *", "pytest *", "go test *", "ruff check *",
  "git status *", "git diff *", "git log *", "git show *", "ls *", "pwd", "cat *", "head *", "tail *", "wc *"]
read_tools = ["Read", "Glob", "Grep", "LS"]
ask = ["git push", "--force", "-f", "rm", "rmdir", "unlink", "sudo", "deploy", "vercel", "gcloud", "fly", "npm publish", "npm install", "npm i",
  "npm ci", "pnpm add", "yarn add", "pip install", "brew", "cargo install", "curl", "wget", "ssh", "scp", "rsync", "chmod", "chown", "kill",
  "git reset", "git clean", "git checkout", "git rebase", "stripe",
  "~--pre", "~-exec", "~-toolexec", "~--config", "~--output", "~--fix", "~--ext-diff", "~--hostname-bin", "~--search-zip",
  "~.env", "~secret", "~id_rsa", "~id_ed25519", "~id_ecdsa", "~.ssh", "~keychain", "~token", "~.pem", "~.key", "~.p12", "~.pfx",
  "~.npmrc", "~.netrc", "~.pypirc", "~credential", "~password", "~.aws", "~.kube", "~.gnupg", "~.git/", "~service-account"]

# Questions an agent leaves in the Claude desktop app's next-step box. bleep may send the
# agent's own first button for you when that button has a word from answer_auto, and
# neither the button nor the question has a word from answer_ask. Whole words, any case.
answer_auto = ["start", "plan", "write", "native", "run", "build", "rebuild", "fix", "add", "rename", "save", "commit", "push", "open pr",
  "open a pr", "check", "continue", "carry on", "go on", "merge", "rebase", "next", "approve", "approved", "verify", "update"]
answer_ask = ["ship", "deploy", "release", "prod", "production", "live", "vercel", "public", "open source", "publish", "delete", "remove",
  "clear", "reset", "drop", "wipe", "undone", "install", "uninstall", "quit", "stop", "kill", "pay", "buy", "purchase", "send", "email",
  "post", "force", "secret", "token", "password", "credential", "feel", "feels", "look", "looks", "works", "test first", "try", "tried",
  "review", "not yet"]
"#;

#[derive(Clone, Debug, PartialEq)]
pub enum RuleVerdict {
    Allow { rule: String },
    NeedsYou { why: String },
}

/// Reads `agent-rules.toml`, writing the defaults when it is missing. A file that does not
/// parse approves nothing, so every ask goes to me.
pub fn load_rules(path: &Path) -> AgentRules {
    let defaults: AgentRules = toml::from_str(DEFAULT_RULES).unwrap_or_default();
    match std::fs::read_to_string(path) {
        Ok(text) => match text.parse::<toml::Table>().and_then(|t| Ok(((t.contains_key("ask"), t.contains_key("answer_auto"), t.contains_key("answer_ask")), toml::from_str::<AgentRules>(&text)?))) {
            // A file without an ask list keeps the default one: leaving it out must not
            // turn the safety list off. Files from before auto-answer get its lists too.
            Ok((has, rules)) => {
                let mut r = if has.0 { rules } else { AgentRules { ask: defaults.ask.clone(), ..rules } };
                if !has.1 {
                    r.answer_auto = defaults.answer_auto.clone();
                }
                if !has.2 {
                    r.answer_ask = defaults.answer_ask.clone();
                }
                r
            }
            Err(e) => {
                eprintln!("bleep: {} does not parse, so every agent ask goes to you: {e}", path.display());
                AgentRules { auto: vec![], read_tools: vec![], answer_auto: vec![], ..defaults }
            }
        },
        Err(_) => {
            if let Err(e) = std::fs::write(path, DEFAULT_RULES) {
                eprintln!("bleep: could not write {}: {e}", path.display());
            }
            defaults
        }
    }
}

/// Splits a shell command into simple commands (on `&&`, `||`, `;`, `|` and newlines),
/// each a list of words with quotes removed. `None` when the command uses anything the
/// rules cannot judge: substitutions, variables, redirects (except `2>&1`), background
/// `&`, subshells, escapes, comments, a leading `NAME=value`, or unbalanced quotes.
pub fn split_command(cmd: &str) -> Option<Vec<Vec<String>>> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut segs: Vec<Vec<String>> = vec![];
    let mut seg: Vec<String> = vec![];
    let mut tok = String::new();
    let mut in_tok = false;
    let mut i = 0;
    let end_tok = |seg: &mut Vec<String>, tok: &mut String, in_tok: &mut bool| {
        if *in_tok {
            seg.push(std::mem::take(tok));
            *in_tok = false;
        }
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' => {
                let close = chars[i + 1..].iter().position(|&x| x == '\'')? + i + 1;
                tok.extend(&chars[i + 1..close]);
                in_tok = true;
                i = close;
            }
            '"' => {
                let close = chars[i + 1..].iter().position(|&x| x == '"')? + i + 1;
                let inner = &chars[i + 1..close];
                if inner.iter().any(|&x| matches!(x, '$' | '`' | '\\')) {
                    return None;
                }
                tok.extend(inner);
                in_tok = true;
                i = close;
            }
            ' ' | '\t' => end_tok(&mut seg, &mut tok, &mut in_tok),
            ';' | '\n' | '\r' | '|' | '&' => {
                let two = chars.get(i + 1).copied();
                if c == '&' && two != Some('&') {
                    return None;
                }
                if (c == '&' && two == Some('&')) || (c == '|' && two == Some('|')) {
                    i += 1;
                }
                end_tok(&mut seg, &mut tok, &mut in_tok);
                if !seg.is_empty() {
                    segs.push(std::mem::take(&mut seg));
                }
            }
            '>' => {
                // Only "2>&1" as its own word: merging stderr into stdout is harmless.
                let rest: String = chars[i..].iter().take(3).collect();
                let starts_word = tok == "2";
                let ends_word = chars.get(i + 3).is_none_or(|x| x.is_whitespace() || *x == '|');
                if !(starts_word && rest == ">&1" && ends_word) {
                    return None;
                }
                tok.clear();
                in_tok = false;
                i += 2;
            }
            '$' | '`' | '\\' | '<' | '(' | ')' => return None,
            // Globs and brace expansion run after bleep looks: `cat .*` can read `.env`.
            '*' | '?' | '[' | ']' | '{' | '}' => return None,
            // zsh: `=cmd` expands to a path, `^` negates a glob.
            '#' | '=' | '^' if !in_tok => return None,
            _ => {
                tok.push(c);
                in_tok = true;
            }
        }
        i += 1;
    }
    end_tok(&mut seg, &mut tok, &mut in_tok);
    if !seg.is_empty() {
        segs.push(seg);
    }
    let assignment = |w: &str| w.split_once('=').is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.starts_with(|c: char| c.is_ascii_digit()));
    if segs.iter().any(|s| assignment(&s[0])) {
        return None;
    }
    Some(segs)
}

const SHELL_TOOLS: &[&str] = &["Bash", "commandExecution"];

/// A folder that can stand for "the repo": absolute, and not `/` or my home folder.
fn repo_folder(cwd: &str) -> bool {
    let p = normalize(Path::new(cwd));
    p.is_absolute() && p != Path::new("/") && dirs::home_dir().is_none_or(|h| normalize(&h) != p)
}

pub fn check(rules: &AgentRules, ask: &Ask) -> RuleVerdict {
    let needs = |why: &str| RuleVerdict::NeedsYou { why: why.to_string() };
    if ask.sandbox_off {
        return needs("wants to run outside the sandbox");
    }
    if !repo_folder(&ask.cwd) {
        return needs("no safe repo folder");
    }
    if rules.read_tools.iter().any(|t| t == &ask.tool_name) {
        let words: Vec<String> = ask.paths.clone();
        if let Some(hit) = ask_hit(rules, &words) {
            return needs(&format!("matches \"{hit}\""));
        }
        if ask.paths.iter().any(|p| !inside(&ask.cwd, p)) {
            return needs("reads outside the repo");
        }
        return RuleVerdict::Allow { rule: format!("{} inside the repo", ask.tool_name) };
    }
    if !SHELL_TOOLS.contains(&ask.tool_name.as_str()) {
        return needs(&format!("{} always asks", ask.tool_name));
    }
    let Some(cmd) = ask.command.as_deref().filter(|c| !c.trim().is_empty()) else { return needs("no command") };
    let Some(segs) = split_command(cmd) else { return needs("the command uses shell features bleep does not judge") };
    if segs.is_empty() {
        return needs("no command");
    }
    for seg in &segs {
        if let Some(hit) = ask_hit(rules, seg) {
            return needs(&format!("matches \"{hit}\""));
        }
        if seg.iter().any(|w| path_parts(w).iter().any(|p| !inside(&ask.cwd, p))) {
            return needs("touches a path outside the repo");
        }
    }
    let mut first = None;
    for seg in &segs {
        let Some(rule) = rules.auto.iter().find(|r| auto_match(r, seg)) else {
            return needs(&format!("\"{}\" is not on the auto list", seg.join(" ")));
        };
        first.get_or_insert(rule.clone());
    }
    RuleVerdict::Allow { rule: first.unwrap_or_default() }
}

/// The first `ask` entry found in these words.
fn ask_hit<'a>(rules: &'a AgentRules, words: &[String]) -> Option<&'a str> {
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    rules.ask.iter().map(String::as_str).find(|entry| {
        let e = entry.to_lowercase();
        if let Some(sub) = e.strip_prefix('~') {
            return lower.iter().any(|w| w.contains(sub));
        }
        let parts: Vec<&str> = e.split_whitespace().collect();
        !parts.is_empty() && lower.windows(parts.len()).any(|win| win.iter().zip(&parts).all(|(a, b)| a == b))
    })
}

fn auto_match(rule: &str, seg: &[String]) -> bool {
    let parts: Vec<&str> = rule.split_whitespace().collect();
    match parts.split_last() {
        Some((&"*", prefix)) => seg.len() >= prefix.len() && seg.iter().zip(prefix).all(|(a, b)| a == b),
        _ => seg.len() == parts.len() && seg.iter().zip(&parts).all(|(a, b)| a == b),
    }
}

/// The parts of a word that name a path bleep must check: the word itself, a value after
/// `=`, and a path glued to a flag (`-f/etc/x`).
fn path_parts(w: &str) -> Vec<String> {
    let mut out = vec![];
    let mut consider = |p: &str| {
        if p.starts_with('/') || p.contains('~') || p.contains("..") {
            out.push(p.to_string());
        }
    };
    consider(w);
    if let Some((_, v)) = w.split_once('=') {
        consider(v);
    }
    if w.starts_with('-') {
        if let Some(at) = w.find(['/', '~']) {
            consider(&w[at..]);
        }
    }
    out
}

/// True when `p` (absolute, or relative to `cwd`) stays inside `cwd`. A `~` is never inside.
fn inside(cwd: &str, p: &str) -> bool {
    if p.contains('~') {
        return false;
    }
    let base = normalize(Path::new(cwd));
    if !base.is_absolute() {
        return false;
    }
    let full = if p.starts_with('/') { normalize(Path::new(p)) } else { normalize(&base.join(p)) };
    full.starts_with(&base)
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "/Users/me/code/shop";

    fn rules() -> AgentRules {
        toml::from_str(DEFAULT_RULES).unwrap()
    }

    fn bash(cmd: &str) -> Ask {
        Ask { kind: AskKind::Permission, tool_name: "Bash".into(), command: Some(cmd.into()), paths: vec![], cwd: REPO.into(), sandbox_off: false }
    }

    fn tool(name: &str, paths: &[&str]) -> Ask {
        Ask { kind: AskKind::Permission, tool_name: name.into(), command: None, paths: paths.iter().map(|p| p.to_string()).collect(), cwd: REPO.into(), sandbox_off: false }
    }

    fn allowed(a: &Ask) -> bool {
        matches!(check(&rules(), a), RuleVerdict::Allow { .. })
    }

    #[test]
    fn npm_test_is_allowed_with_rule_name() {
        assert_eq!(check(&rules(), &bash("npm test")), RuleVerdict::Allow { rule: "npm test".into() });
    }

    #[test]
    fn cargo_test_with_args_is_allowed() {
        assert_eq!(check(&rules(), &bash("cargo test --lib agentwatch")), RuleVerdict::Allow { rule: "cargo test *".into() });
        assert!(allowed(&bash("npx vitest run src/cart.test.ts")));
        assert!(allowed(&bash("git status")));
        assert!(allowed(&bash("cat 'src/a b.ts'")));
    }

    #[test]
    fn piped_to_tail_is_allowed() {
        assert!(allowed(&bash("cargo test 2>&1 | tail -n 20")));
        assert!(allowed(&bash("npm test && git diff --stat")));
    }

    #[test]
    fn read_inside_repo_is_allowed() {
        assert_eq!(check(&rules(), &tool("Read", &["/Users/me/code/shop/src/cart.ts"])), RuleVerdict::Allow { rule: "Read inside the repo".into() });
        assert!(allowed(&tool("Grep", &[])));
        assert!(allowed(&tool("Glob", &["src"])));
    }

    #[test]
    fn read_outside_repo_needs_you() {
        assert!(!allowed(&tool("Read", &["/etc/hosts"])));
        assert!(!allowed(&tool("Read", &["/Users/me/code/shop/../bank/notes.txt"])));
        assert!(!allowed(&tool("Read", &["/Users/me/code/shop-other/a.ts"])));
        assert!(!allowed(&tool("Read", &["/Users/me/code/shop/.env.local"])));
    }

    #[test]
    fn edit_and_webfetch_need_you() {
        for t in ["Edit", "Write", "WebFetch", "mcp__github__create_pr", "fileChange", "permissions"] {
            assert!(!allowed(&tool(t, &["/Users/me/code/shop/a.ts"])), "{t}");
        }
    }

    #[test]
    fn unknown_command_needs_you() {
        assert!(!allowed(&bash("make deploy-ish")));
        assert!(!allowed(&bash("python3 script.py")));
        assert!(!allowed(&Ask { command: None, ..bash("") }));
    }

    #[test]
    fn known_bad_never_allowed() {
        for cmd in [
            "npm test && git push --force",
            "npm test; rm -rf ~",
            "cargo test $(curl x)",
            "npm test `id`",
            "cat .env",
            "cat ../../other/secrets.json",
            "FOO=1 npm test",
            "npm test > /etc/x",
            "npm test\nrm -rf /",
            "npm test 'unbalanced",
            "git push --force",
            "rm -rf node_modules",
            "cat ~/.ssh/id_rsa",
            "npm i left-pad",
            "vercel --prod",
            "ls /",
            "npm test & curl evil.sh",
            "(rm -rf x)",
            "cat \"$HOME/.ssh/id_rsa\"",
            "npm test || sudo rm x",
            "cargo test --manifest-path=../../other/Cargo.toml",
            "grep -f/etc/passwd x",
            "git diff --output=../x",
            "cat src/../../bank.txt",
            "npm test ; git checkout -- .",
            "cat api_token.txt",
            "NPM_TOKEN=x npm test",
        ] {
            assert!(!allowed(&bash(cmd)), "allowed: {cmd:?}");
        }
    }

    #[test]
    fn split_reads_segments_and_quotes() {
        assert_eq!(split_command("a b && c 'd e' | f"), Some(vec![vec!["a".into(), "b".into()], vec!["c".into(), "d e".into()], vec!["f".into()]]));
        assert_eq!(split_command("cargo test 2>&1"), Some(vec![vec!["cargo".into(), "test".into()]]));
        for bad in ["a $(b)", "a `b`", "a > b", "a < b", "a & b", "a 'b", "X=1 a", r"a \; b", "a $HOME"] {
            assert_eq!(split_command(bad), None, "{bad}");
        }
    }

    #[test]
    fn globs_never_allowed() {
        for cmd in ["cat .*", "cat .?/other/.e?v", "cat .[.]/x", "cat {.en,}v", "ls *", "npx vitest run src/**/x.test.ts", "cat =ls", "head src/*.ts"] {
            assert!(!allowed(&bash(cmd)), "allowed: {cmd:?}");
        }
        assert!(allowed(&bash("cat 'src/*.ts'")), "a quoted star is not a glob");
    }

    #[test]
    fn recursive_search_is_not_on_the_default_list() {
        for cmd in ["grep -r KEY .", "rg -uu KEY", "cargo test 2>&1 | grep FAIL"] {
            assert!(!allowed(&bash(cmd)), "allowed: {cmd:?}");
        }
    }

    #[test]
    fn empty_or_wide_cwd_needs_you() {
        let home = dirs::home_dir().unwrap().display().to_string();
        for cwd in ["", "/", "relative/repo", home.as_str()] {
            assert!(!allowed(&Ask { cwd: cwd.into(), ..bash("npm test") }), "cwd {cwd:?}");
            assert!(!allowed(&Ask { cwd: cwd.into(), ..tool("Read", &["/etc/hosts"]) }), "cwd {cwd:?}");
        }
    }

    #[test]
    fn flags_that_run_or_write_need_you() {
        for cmd in ["rg --pre bash .", "go test -exec 'curl x' ./...", "go test -toolexec x ./...", "cargo test --config 'target.x.runner=\"sh\"'", "git diff --output=src/main.rs", "git log --output src/main.rs", "cargo clippy --fix", "ruff check --fix", "git diff --ext-diff"] {
            assert!(!allowed(&bash(cmd)), "allowed: {cmd:?}");
        }
    }

    #[test]
    fn sandbox_off_needs_you() {
        assert!(!allowed(&Ask { sandbox_off: true, ..bash("npm test") }));
    }

    #[test]
    fn more_secret_files_need_you() {
        for cmd in ["cat .npmrc", "cat config/credentials.json", "cat certs/server.key", "cat .git/config", "cat ~/.aws/credentials", "cat deploy/service-account.json", "cat .netrc", "cat keys/id_ed25519", "cat db/password.txt"] {
            assert!(!allowed(&bash(cmd)), "allowed: {cmd:?}");
        }
    }

    #[test]
    fn partial_rules_file_keeps_the_ask_list() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("agent-rules.toml");
        std::fs::write(&p, "auto = [\"git *\"]\n").unwrap();
        let r = load_rules(&p);
        assert_eq!(r.ask, rules().ask);
        assert!(!matches!(check(&r, &bash("git push --force")), RuleVerdict::Allow { .. }));
        assert!(matches!(check(&r, &bash("git status")), RuleVerdict::Allow { .. }));
    }

    #[test]
    fn bad_rules_file_allows_nothing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("agent-rules.toml");
        std::fs::write(&p, "auto = [this is not toml").unwrap();
        let r = load_rules(&p);
        assert!(r.auto.is_empty());
        assert!(!matches!(check(&r, &bash("npm test")), RuleVerdict::Allow { .. }));
        assert!(!matches!(check(&r, &tool("Read", &["/Users/me/code/shop/a.ts"])), RuleVerdict::Allow { .. }));
    }

    #[test]
    fn old_rules_file_gets_the_answer_lists() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("agent-rules.toml");
        std::fs::write(&p, "auto = [\"npm test\"]\nask = [\"rm\"]\n").unwrap();
        let r = load_rules(&p);
        assert_eq!((r.answer_auto, r.answer_ask), (rules().answer_auto, rules().answer_ask));
        std::fs::write(&p, "answer_auto = []\n").unwrap();
        assert!(load_rules(&p).answer_auto.is_empty(), "my own empty list wins");
        std::fs::write(&p, "auto = [this is not toml").unwrap();
        assert!(load_rules(&p).answer_auto.is_empty(), "a bad file answers nothing");
    }

    #[test]
    fn missing_rules_file_gets_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("agent-rules.toml");
        assert_eq!(load_rules(&p), rules());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), DEFAULT_RULES);
    }
}
