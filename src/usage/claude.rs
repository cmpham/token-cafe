//! Attribution uses saved transcript evidence, never today's CLI login.
use super::*;
use std::collections::{BTreeSet, HashSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Surface {
    Desktop,
    Cli,
    Unassigned,
    Codex,
}

impl Surface {
    pub fn name(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Cli => "cli",
            Self::Unassigned => "unassigned",
            Self::Codex => "codex",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "desktop" => Some(Self::Desktop),
            "cli" => Some(Self::Cli),
            "unassigned" => Some(Self::Unassigned),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Group {
    pub surface: Surface,
    pub account_id: Option<String>,
    pub organization_id: Option<String>,
}

impl Group {
    pub(super) fn unknown(surface: Surface) -> Self {
        Self { surface, account_id: None, organization_id: None }
    }
}

#[derive(Clone)]
pub(super) struct Response {
    pub key: String,
    pub day: String,
    pub tally: Tally,
    pub output: i64,
    pub group: Group,
    conflicted: bool,
}

impl Response {
    pub fn json(&self) -> Value {
        json!([
            self.key,
            self.day,
            self.tally.tokens,
            self.tally.cost,
            self.output,
            self.group.surface.name(),
            self.group.account_id,
            self.group.organization_id,
            self.conflicted
        ])
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            key: v.get(0)?.as_str()?.to_owned(),
            day: v.get(1)?.as_str()?.to_owned(),
            tally: Tally { tokens: v.get(2)?.as_i64()?, cost: v.get(3)?.as_f64()? },
            output: v.get(4)?.as_i64()?,
            group: Group {
                surface: Surface::parse(v.get(5)?.as_str()?)?,
                account_id: v.get(6)?.as_str().map(str::to_owned),
                organization_id: v.get(7)?.as_str().map(str::to_owned),
            },
            conflicted: v.get(8)?.as_bool()?,
        })
    }
}

/// A fork can repeat an entire parent transcript. Count each API response once,
/// keep its final streamed usage, and never choose between conflicting identities.
pub(super) fn merge(sink: &mut HashMap<String, Response>, response: Response) {
    match sink.entry(response.key.clone()) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(response);
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            let old = entry.get_mut();
            let conflicted = old.conflicted || response.conflicted || old.group != response.group;
            let surface =
                if old.group.surface == response.group.surface { old.group.surface } else { Surface::Unassigned };
            if response.output > old.output {
                *old = response;
            }
            if conflicted {
                old.group = Group::unknown(surface);
                old.conflicted = true;
            }
        }
    }
}

pub(super) fn parse(path: &Path) -> std::io::Result<Vec<Response>> {
    let mut best = HashMap::new();
    let mut owners: HashMap<String, BTreeSet<(String, Option<String>)>> = HashMap::new();
    let mut response_sessions = HashMap::new();
    let fallback_session = path.file_stem().and_then(|p| p.to_str()).unwrap_or("");
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut line = Vec::new();
    let mut line_number = 0;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        line_number += 1;
        let ownership = contains(&line, b"\"bridge-session\"");
        if !ownership && !(contains(&line, b"\"usage\"") && contains(&line, b"\"assistant\"")) {
            continue;
        }
        let Ok(d) = serde_json::from_slice::<Value>(&line) else { continue };
        let session = d["sessionId"].as_str().unwrap_or(fallback_session);
        if d["type"] == "bridge-session" {
            if let Some(account) = d["ownerAccountUuid"].as_str().filter(|s| !s.is_empty()) {
                owners.entry(session.to_owned()).or_default().insert((
                    account.to_owned(),
                    d["ownerOrganizationUuid"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
                ));
            }
            continue;
        }
        if d["type"] != "assistant" {
            continue;
        }
        let msg = &d["message"];
        let usage = &msg["usage"];
        if !usage.as_object().is_some_and(|o| !o.is_empty()) {
            continue;
        }
        let Some(day) = local_day(d.get("timestamp")) else { continue };
        let model = msg["model"].as_str().unwrap_or("");
        if model.is_empty() || model.starts_with('<') {
            continue;
        }
        let message_id = msg["id"].as_str().filter(|s| !s.is_empty());
        let request_id = d["requestId"].as_str().filter(|s| !s.is_empty());
        let key = if message_id.is_some() || request_id.is_some() {
            json!([message_id, request_id]).to_string()
        } else {
            // Missing IDs cannot safely be collapsed, even within the same file.
            json!([path, line_number]).to_string()
        };
        let inp = num(usage.get("input_tokens"));
        let read = num(usage.get("cache_read_input_tokens"));
        let out = num(usage.get("output_tokens"));
        let creation = usage.get("cache_creation");
        let mut w5 = num(creation.and_then(|c| c.get("ephemeral_5m_input_tokens")));
        let w1h = num(creation.and_then(|c| c.get("ephemeral_1h_input_tokens")));
        if w5 == 0 && w1h == 0 {
            w5 = num(usage.get("cache_creation_input_tokens"));
        }
        let mut tally = Tally { tokens: inp + w5 + w1h + read + out, cost: 0.0 };
        if let Some([pin, pout, pread]) = price(CLAUDE_PRICES, model) {
            tally.cost = (inp as f64 * pin
                + w5 as f64 * pin * 1.25
                + w1h as f64 * pin * 2.0
                + read as f64 * pread
                + out as f64 * pout)
                / M;
        }
        let surface = match d["entrypoint"].as_str() {
            Some("claude-desktop") => Surface::Desktop,
            Some("cli") => Surface::Cli,
            _ => Surface::Unassigned,
        };
        response_sessions.entry(key.clone()).or_insert_with(HashSet::new).insert(session.to_owned());
        merge(&mut best, Response { key, day, tally, output: out, group: Group::unknown(surface), conflicted: false });
    }
    for response in best.values_mut() {
        if response.conflicted {
            continue;
        }
        let sessions = &response_sessions[&response.key];
        if sessions.len() != 1 {
            continue;
        }
        let Some(owners) = owners.get(sessions.iter().next().unwrap()) else { continue };
        if owners.len() == 1 {
            let (account, organization) = owners.first().unwrap();
            response.group.account_id = Some(account.clone());
            response.group.organization_id = organization.clone();
        }
    }
    Ok(best.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(id: Option<&str>, surface: Option<&str>, session: &str, output: i64) -> Value {
        json!({"type":"assistant", "timestamp":"2026-10-10T12:00:00Z", "sessionId":session,
            "entrypoint":surface, "message":{"id":id,"model":"claude-sonnet-5",
            "usage":{"input_tokens":10,"output_tokens":output}}})
    }
    fn owner(session: &str, org: &str) -> Value {
        json!({"type":"bridge-session", "sessionId":session,
            "ownerAccountUuid":"user", "ownerOrganizationUuid":org})
    }
    fn fixture(rows: &[Value]) -> (crate::collect::TestDir, PathBuf) {
        let dir = crate::collect::TestDir::new();
        let path = dir.0.join("session.jsonl");
        fs::write(&path, rows.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
        (dir, path)
    }

    #[test]
    fn saved_surfaces_and_owners_do_not_use_the_current_login() {
        let (_dir, path) = fixture(&[
            owner("desktop", "personal"),
            owner("cli", "work"),
            usage(Some("one"), Some("claude-desktop"), "desktop", 2),
            usage(Some("two"), Some("cli"), "cli", 3),
            usage(Some("legacy"), None, "unknown", 4),
        ]);
        let rows = parse(&path).unwrap();
        assert_eq!(rows.len(), 3);
        let desktop = rows.iter().find(|r| r.group.surface == Surface::Desktop).unwrap();
        assert_eq!(desktop.group.organization_id.as_deref(), Some("personal"));
        assert_eq!(desktop.tally.tokens, 12);
        let cli = rows.iter().find(|r| r.group.surface == Surface::Cli).unwrap();
        assert_eq!(cli.group.organization_id.as_deref(), Some("work"));
        assert_eq!(cli.tally.tokens, 13);
        let unknown = rows.iter().find(|r| r.group.surface == Surface::Unassigned).unwrap();
        assert_eq!(unknown.group.account_id, None);
    }

    #[test]
    fn mixed_or_switched_session_ownership_remains_unassigned() {
        let (_dir, path) = fixture(&[
            owner("same", "personal"),
            usage(Some("one"), Some("cli"), "same", 2),
            owner("same", "work"),
            usage(Some("two"), Some("claude-desktop"), "same", 3),
        ]);
        let rows = parse(&path).unwrap();
        assert!(rows.iter().all(|r| r.group.account_id.is_none()));
        assert!(rows.iter().any(|r| r.group.surface == Surface::Desktop));
        assert!(rows.iter().any(|r| r.group.surface == Surface::Cli));
    }

    #[test]
    fn fork_copies_and_streamed_responses_are_counted_once_globally() {
        let (_a, path_a) = fixture(&[owner("s", "personal"), usage(Some("one"), Some("cli"), "s", 2)]);
        let (_b, path_b) = fixture(&[owner("s", "personal"), usage(Some("one"), Some("cli"), "s", 5)]);
        let mut all = HashMap::new();
        for path in [&path_a, &path_b] {
            for row in parse(path).unwrap() {
                merge(&mut all, row);
            }
        }
        assert_eq!(all.len(), 1);
        let row = all.values().next().unwrap();
        assert_eq!(row.tally.tokens, 15);
        assert_eq!(row.group.organization_id.as_deref(), Some("personal"));
        assert_eq!(Response::from_json(&row.json()).unwrap().tally, row.tally);
    }

    #[test]
    fn absent_response_ids_are_not_collapsed_and_conflicts_stay_unknown() {
        let (_dir, path) = fixture(&[usage(None, Some("cli"), "s", 2), usage(None, Some("cli"), "s", 3)]);
        assert_eq!(parse(&path).unwrap().len(), 2);
        let (_a, path_a) = fixture(&[owner("s", "personal"), usage(Some("one"), Some("cli"), "s", 2)]);
        let (_b, path_b) = fixture(&[owner("s", "work"), usage(Some("one"), Some("cli"), "s", 2)]);
        let mut all = HashMap::new();
        for path in [&path_a, &path_b, &path_a] {
            for row in parse(path).unwrap() {
                merge(&mut all, row);
            }
        }
        assert_eq!(all.len(), 1);
        let row = all.values().next().unwrap();
        assert_eq!(row.group.surface, Surface::Cli);
        assert_eq!(row.group.account_id, None);
    }
}
