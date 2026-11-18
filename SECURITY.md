# Security policy

bleep reads your mail and calendar and can approve actions in your coding agents. Security reports matter a lot here.

## Report a problem

Please do not open a public issue. Use GitHub's private report instead:
[Report a vulnerability](https://github.com/AviroopPaul/bleep/security/advisories/new).

Include what you found, the steps to reproduce it, and the bleep version. You will get a reply within 7 days.

## In scope

- An approval rule that lets a risky ask through (push, deploy, delete, secrets, money, sending mail).
- Text in an email, calendar event, file or web page that makes bleep act without the user's yes (prompt injection).
- Tokens, keys or mail content that leak to disk outside `~/.bleep/`, to logs, or off the Mac.
- The install script, the updater, or the Homebrew cask installing something other than the signed release.

## Supported versions

Only the latest release gets fixes. bleep updates itself, so most users are on it within a day.
