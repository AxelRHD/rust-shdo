//! Sends a task as a push message via ntfy (https://ntfy.sh), e.g. a shopping list.
//!
//! Plain text only: the ntfy apps show Markdown task lists as `[ ] item` and
//! notifications never render Markdown, so checkboxes become ☐ / ☑.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rust_i18n::t;

use crate::task::Task;

/// Server used when `[ntfy]` has no `server`.
pub const DEFAULT_SERVER: &str = "https://ntfy.sh";

/// `[ntfy]` in config.toml; sending is enabled once a topic is set.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct NtfyConfig {
    /// e.g. `https://ntfy.example.org`; default [`DEFAULT_SERVER`].
    pub server: Option<String>,
    /// Works like a password on ntfy.sh, so a long random one.
    pub topic: Option<String>,
    /// Access token for protected topics (`tk_…`).
    pub token: Option<String>,
}

/// JSON body for publishing; JSON keeps umlauts in the title intact, unlike headers.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct Message {
    topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

/// Server without trailing slash; `https://` is added when missing.
pub fn normalize_server(server: &str) -> Result<String> {
    let s = server.trim().trim_end_matches('/');
    if s.is_empty() || s.contains(char::is_whitespace) {
        bail!(t!("ntfy.bad_server", server = server));
    }
    Ok(match s.contains("://") {
        true => s.to_string(),
        false => format!("https://{s}"),
    })
}

/// A new random topic such as `shdo-k3Vq…` (24 random characters from /dev/urandom).
/// On ntfy.sh the topic is the only protection, so it must not be guessable.
pub fn generate_topic() -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = [0u8; 64];
    let urandom = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes));
    if urandom.is_err() {
        bytes.fill_with(|| fastrand::u8(..));
    }
    // 248 = 4 * 62: dropping larger bytes keeps every character equally likely
    let random: String = bytes
        .iter()
        .filter(|b| **b < 248)
        .take(24)
        .map(|b| CHARS[usize::from(*b) % CHARS.len()] as char)
        .collect();
    format!("shdo-{random}")
}

/// Topics are letters, digits, `-` and `_` (at most 64).
pub fn check_topic(topic: &str) -> Result<&str> {
    let t = topic.trim();
    let ok = !t.is_empty()
        && t.len() <= 64
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        bail!(t!("ntfy.bad_topic", topic = topic));
    }
    Ok(t)
}

impl NtfyConfig {
    /// The topic if one is set, i.e. sending is enabled.
    pub fn topic(&self) -> Option<&str> {
        self.topic
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }

    pub fn server(&self) -> Result<String> {
        normalize_server(self.server.as_deref().unwrap_or(DEFAULT_SERVER))
    }

    pub fn send(&self, task: &Task) -> Result<()> {
        let Some(topic) = self.topic() else {
            bail!(t!("msg.push_not_set_up"));
        };
        let topic = check_topic(topic)?;
        let base = self.server()?;
        let msg = message(task, topic);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut req = agent.post(&base);
        if let Some(token) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let mut res = req.send_json(&msg).context(t!("ntfy.failed"))?;
        let status = res.status();
        if status.is_success() {
            return Ok(());
        }
        // ntfy explains errors as {"error": "..."}
        let detail = res
            .body_mut()
            .read_json::<serde_json::Value>()
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or_else(|| status.to_string());
        Err(anyhow!("{}: {detail}", t!("ntfy.failed")))
    }
}

/// Title, notes with ☐/☑ instead of Markdown checkboxes, priority and tags.
pub fn message(task: &Task, topic: &str) -> Message {
    let body = plain_checklist(task.note());
    // Without notes the title alone is the message
    let (title, message) = match body.is_empty() {
        true => (None, task.title.clone()),
        false => (Some(task.title.clone()), body),
    };
    Message {
        topic: topic.into(),
        title,
        message,
        priority: match task.prio {
            Some(1) => Some(4),
            Some(3) => Some(2),
            _ => None,
        },
        tags: task.tags.clone(),
    }
}

/// `- [ ] Milch` → `☐ Milch`, `- [x] Eier` → `☑ Eier`; other lines stay as they are.
fn plain_checklist(note: &str) -> String {
    note.lines()
        .map(|line| {
            let indent = &line[..line.len() - line.trim_start().len()];
            let l = line.trim_start();
            let rest = l.strip_prefix("- ").or_else(|| l.strip_prefix("* "));
            match rest.and_then(|r| r.get(..3).map(|b| (b, &r[3..]))) {
                Some(("[ ]", item)) => format!("{indent}☐{item}"),
                Some(("[x]" | "[X]", item)) => format!("{indent}☑{item}"),
                _ => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(raw: &str) -> Task {
        Task::parse("a1b2", "a1b2.md".into(), raw).unwrap()
    }

    #[test]
    fn checklist_becomes_plain_text() {
        let t = task(
            "+++\ntitle = \"Einkauf\"\nstatus = \"todo\"\nprio = 1\ntags = [\"shopping_cart\"]\n+++\n\n\
             - [ ] Milch\n- [x] Eier\n  * [ ] Bio\nbeim Bäcker vorbei\n- normal\n",
        );
        let msg = message(&t, "topic");
        assert_eq!(msg.title.as_deref(), Some("Einkauf"));
        assert_eq!(
            msg.message,
            "☐ Milch\n☑ Eier\n  ☐ Bio\nbeim Bäcker vorbei\n- normal"
        );
        assert_eq!(msg.priority, Some(4));
        assert_eq!(msg.tags, ["shopping_cart"]);
    }

    #[test]
    fn title_only_without_notes() {
        let msg = message(&task("+++\ntitle = \"Anrufen\"\n+++\n"), "t");
        assert_eq!(msg.title, None);
        assert_eq!(msg.message, "Anrufen");
        assert_eq!(msg.priority, None);
    }

    #[test]
    fn generated_topics_are_valid_and_differ() {
        let a = generate_topic();
        assert!(a.starts_with("shdo-"));
        assert_eq!(a.len(), 5 + 24);
        assert_eq!(check_topic(&a).unwrap(), a);
        assert_ne!(a, generate_topic());
    }

    #[test]
    fn server_and_topic_are_checked() {
        assert_eq!(NtfyConfig::default().server().unwrap(), DEFAULT_SERVER);
        assert_eq!(
            normalize_server("push.example.org/ntfy/").unwrap(),
            "https://push.example.org/ntfy"
        );
        assert_eq!(
            normalize_server("http://localhost:8080").unwrap(),
            "http://localhost:8080"
        );
        assert!(normalize_server(" ").is_err());
        assert_eq!(check_topic(" shdo-abc_1 ").unwrap(), "shdo-abc_1");
        assert!(check_topic("a b").is_err());
        assert!(check_topic(&"x".repeat(65)).is_err());
        let cfg = NtfyConfig {
            topic: Some("  ".into()),
            ..NtfyConfig::default()
        };
        assert_eq!(cfg.topic(), None);
    }
}
