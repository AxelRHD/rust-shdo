//! Sends a task as a push message via ntfy (https://ntfy.sh), e.g. a shopping list.
//!
//! Plain text only: the ntfy apps show Markdown task lists as `[ ] item` and
//! notifications never render Markdown, so checkboxes become ☐ / ☑.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rust_i18n::t;

use crate::task::Task;

/// `[ntfy]` in config.toml.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NtfyConfig {
    /// Server and topic, e.g. `https://ntfy.sh/my-secret-topic`.
    pub url: String,
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

impl NtfyConfig {
    /// `https://ntfy.sh/topic` → (`https://ntfy.sh`, `topic`).
    fn split(&self) -> Result<(&str, &str)> {
        let url = self.url.trim().trim_end_matches('/');
        match url.rsplit_once('/') {
            Some((base, topic))
                if base.contains("://")
                    && !topic.is_empty()
                    && topic
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
            {
                Ok((base, topic))
            }
            _ => bail!(t!("ntfy.bad_url", url = self.url)),
        }
    }

    pub fn send(&self, task: &Task) -> Result<()> {
        let (base, topic) = self.split()?;
        let msg = message(task, topic);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut req = agent.post(base);
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
    fn url_is_split_into_server_and_topic() {
        let cfg = |url: &str| NtfyConfig {
            url: url.into(),
            token: None,
        };
        let c = cfg("https://ntfy.sh/shdo-abc_1/");
        assert_eq!(c.split().unwrap(), ("https://ntfy.sh", "shdo-abc_1"));
        let c = cfg("https://push.example.org/ntfy/einkauf");
        assert_eq!(
            c.split().unwrap(),
            ("https://push.example.org/ntfy", "einkauf")
        );
        assert!(cfg("https://ntfy.sh/").split().is_err());
        assert!(cfg("ntfy.sh/topic").split().is_err());
        assert!(cfg("https://ntfy.sh/a b").split().is_err());
    }
}
