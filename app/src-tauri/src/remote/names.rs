//! Rules for server names and URLs.
use reqwest::Url;

/// Connector ids a remote server may not take.
pub const BUILTIN: &[&str] = &["bleep", "gmail", "calendar", "drive", "files", "catalog", "memory", "reminders", "agents", "google"];
pub const MAX_NAME: usize = 16;
const SKIP: &[&str] = &["www", "api", "mcp", "app"];

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// A short name from the URL's host: `dayblocks.vercel.app` gives `dayblocks`.
pub fn server_name_from(url: &str) -> String {
    let Ok(u) = Url::parse(url) else { return "server".into() };
    let Some(host) = u.host_str() else { return "server".into() };
    if is_loopback(host) {
        return "local".into();
    }
    let label = host.split('.').find(|l| !SKIP.contains(l)).unwrap_or(host);
    let name: String = label.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).take(MAX_NAME).collect();
    if name.len() < 2 || !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        return "server".into();
    }
    name
}

pub fn check_server_name(name: &str, taken: &[&str]) -> Result<(), String> {
    let ok_chars = name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !(2..=MAX_NAME).contains(&name.len()) || !ok_chars || !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Err(format!("Use 2 to {MAX_NAME} lowercase letters or digits, starting with a letter."));
    }
    if BUILTIN.contains(&name) {
        return Err(format!("\"{name}\" is used by bleep. Pick another name."));
    }
    if taken.contains(&name) {
        return Err(format!("A server named \"{name}\" is already added."));
    }
    Ok(())
}

/// `https://` only, or `http://` on this Mac. No credentials in the URL, and not bleep's own server.
pub fn check_url(url: &str, own_port: Option<u16>) -> Result<Url, String> {
    let u = Url::parse(url.trim()).map_err(|_| "That is not a URL. Paste the full address, for example https://example.com/mcp.".to_string())?;
    let host = u.host_str().unwrap_or("").to_string();
    match u.scheme() {
        "https" => {}
        "http" if is_loopback(&host) => {}
        "http" => return Err("Use an https:// address. Plain http works only for servers on this Mac.".into()),
        _ => return Err("Use an https:// address.".into()),
    }
    if host.is_empty() {
        return Err("That URL has no host.".into());
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("Take the user name and password out of the URL. bleep asks for sign-in separately.".into());
    }
    if is_loopback(&host) && own_port.is_some() && u.port_or_known_default() == own_port {
        return Err("That is bleep's own server.".into());
    }
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_from_url_skips_common_labels() {
        assert_eq!(server_name_from("https://dayblocks.vercel.app/api/mcp"), "dayblocks");
        assert_eq!(server_name_from("https://mcp.linear.app/mcp"), "linear");
        assert_eq!(server_name_from("https://api.www.example.com/x"), "example");
        assert_eq!(server_name_from("http://localhost:3000/api/mcp"), "local");
        assert_eq!(server_name_from("https://my-very-long-service-name.io/mcp"), "myverylongservic");
        assert_eq!(server_name_from("not a url"), "server");
    }

    #[test]
    fn server_name_rules() {
        assert!(check_server_name("dayblocks", &[]).is_ok());
        assert!(check_server_name("d", &[]).is_err(), "too short");
        assert!(check_server_name("abcdefghijklmnopq", &[]).is_err(), "17 chars");
        assert!(check_server_name("Day", &[]).is_err(), "uppercase");
        assert!(check_server_name("day_blocks", &[]).is_err(), "underscore would confuse tool names");
        assert!(check_server_name("9lives", &[]).is_err(), "must start with a letter");
        for b in BUILTIN {
            assert!(check_server_name(b, &[]).is_err(), "{b} is built in");
        }
        assert!(check_server_name("wiki", &["wiki"]).is_err(), "taken");
    }

    #[test]
    fn url_rules() {
        assert!(check_url("https://dayblocks.vercel.app/api/mcp", None).is_ok());
        assert!(check_url("http://localhost:3000/api/mcp", None).is_ok());
        assert!(check_url("http://127.0.0.1:3000/mcp", None).is_ok());
        assert!(check_url("http://[::1]:3000/mcp", None).is_ok());
        assert!(check_url("http://dayblocks.vercel.app/api/mcp", None).is_err(), "plain http off localhost");
        assert!(check_url("ftp://x.io", None).is_err());
        assert!(check_url("https://user:pw@x.io/mcp", None).is_err(), "no credentials in the URL");
        assert!(check_url("nonsense", None).is_err());
        assert!(check_url("http://127.0.0.1:4100/mcp", Some(4100)).is_err(), "bleep's own server");
        assert!(check_url("http://localhost:4100/mcp", Some(4100)).is_err(), "bleep's own server");
        assert!(check_url("http://localhost:4101/mcp", Some(4100)).is_ok());
    }
}
