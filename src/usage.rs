//! This calendar month's tokens and list-price dollars for the tab, Claude
//! Code and Codex summed.
//!
//! Both CLIs keep a full transcript of every session on disk, and every
//! assistant turn carries the token usage the API reported for it:
//!
//!   Claude   ~/.claude/projects/<slug>/<session>.jsonl   ($CLAUDE_CONFIG_DIR)
//!            assistant records -> message.usage, message.model
//!   Codex    ~/.codex/sessions/<y>/<m>/<d>/rollout-*.jsonl   ($CODEX_HOME)
//!            token_usage_record events (new), token_count events (older)
//!
//! The dollars are what the same traffic would have cost on each vendor's
//! API at list price: a measure of what was consumed, not of what was billed.
//! Per-file results are cached by size and mtime, so a re-count only re-reads
//! the transcripts that changed.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{Value, json};

const M: f64 = 1_000_000.0;
const CACHE_VERSION: u64 = 2;

mod claude;
pub use claude::{Group, Surface};

/* ---- price list, US dollars per million tokens ---------------------------
 * Claude: cache writes are 1.25x input for the 5 minute TTL and 2x for the
 * 1 hour TTL; cache reads are 0.1x input, except Fable 5.1 at 0.025x.
 * Codex: cached input and cache writes are quoted directly by OpenAI. */

/// model prefix -> (input, output, cache read)
const CLAUDE_PRICES: &[(&str, [f64; 3])] = &[
    ("claude-fable-5-1", [10.00, 50.00, 0.25]),
    ("claude-mythos-5-1", [10.00, 50.00, 0.25]),
    ("claude-fable-5", [10.00, 50.00, 1.00]),
    ("claude-opus-5-5", [4.00, 20.00, 0.20]),
    ("claude-opus-5", [5.00, 25.00, 0.50]),
    ("claude-opus-4-8", [5.00, 25.00, 0.50]),
    ("claude-opus-4-7", [5.00, 25.00, 0.50]),
    ("claude-opus-4-6", [5.00, 25.00, 0.50]),
    ("claude-sonnet-5-5", [2.00, 10.00, 0.20]),
    ("claude-sonnet-5", [2.00, 10.00, 0.20]),
    ("claude-sonnet-4-6", [3.00, 15.00, 0.30]),
    ("claude-haiku-4-5", [1.00, 5.00, 0.10]),
];

/// model -> (input, output, cached input, cache write)
const CODEX_PRICES: &[(&str, [f64; 4])] = &[
    ("gpt-6-astra", [10.00, 50.00, 1.00, 12.50]),
    ("gpt-5.6-cyber", [12.50, 75.00, 1.25, 15.625]),
    ("gpt-5.6-sol", [4.00, 20.00, 0.40, 5.00]),
    ("gpt-5.6-terra", [2.00, 12.00, 0.20, 2.50]),
    ("gpt-5.6-luna", [0.20, 1.20, 0.02, 0.25]),
    ("gpt-5.5-pro", [30.00, 180.00, 30.00, 37.50]),
    ("gpt-5.5", [5.00, 30.00, 0.50, 6.25]),
    ("gpt-5.4-mini", [0.75, 4.50, 0.075, 0.9375]),
    ("gpt-5.4-nano", [0.20, 1.25, 0.02, 0.25]),
    ("gpt-5.4", [2.50, 15.00, 0.25, 3.125]),
];

/// Longest prefix first: "claude-opus-5" also prefixes "claude-opus-5-5".
fn price<const N: usize>(table: &[(&str, [f64; N])], model: &str) -> Option<[f64; N]> {
    table.iter().filter(|(p, _)| model.starts_with(p)).max_by_key(|(p, _)| p.len()).map(|&(_, v)| v)
}

/// Tokens and dollars for one local day.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Tally {
    pub tokens: i64,
    pub cost: f64,
}

impl Tally {
    fn add(&mut self, o: Tally) {
        self.tokens += o.tokens;
        self.cost += o.cost;
    }
}

/// Local date (YYYY-MM-DD) -> totals.
type Days = BTreeMap<String, Tally>;

fn num(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
        _ => 0,
    }
}

/// An ISO-8601 timestamp or epoch seconds -> local date.
fn local_day(ts: Option<&Value>) -> Option<String> {
    let day = match ts? {
        Value::Number(n) => Local.timestamp_opt(n.as_f64()? as i64, 0).single()?.date_naive(),
        Value::String(s) => match DateTime::parse_from_rfc3339(s) {
            Ok(dt) => dt.with_timezone(&Local).date_naive(),
            Err(_) => NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()?.date(),
        },
        _ => return None,
    };
    Some(day.format("%Y-%m-%d").to_string())
}

fn lines(path: &Path) -> std::io::Result<impl Iterator<Item = Vec<u8>>> {
    let f = BufReader::new(fs::File::open(path)?);
    Ok(f.split(b'\n').map_while(Result::ok))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(hay, needle).is_some()
}

fn charge(sink: &mut Days, day: Option<String>, model: &str, inp: i64, cached: i64, written: i64, out: i64) {
    let Some(day) = day else { return };
    let uncached = (inp - cached - written).max(0);
    let mut t = Tally { tokens: uncached + cached + written + out, cost: 0.0 };
    if let Some([pin, pout, pcached, pwrite]) = price(CODEX_PRICES, model) {
        t.cost = (uncached as f64 * pin + cached as f64 * pcached + written as f64 * pwrite + out as f64 * pout) / M;
    }
    sink.entry(day).or_default().add(t);
}

/// Newer Codex writes one `token_usage_record` per API response. Older
/// versions wrote `token_count` events carrying a running total for the
/// session, so those are differenced. A current rollout contains *both* kinds
/// for the same responses, so the two are accumulated separately and the
/// modern one wins; adding them double-counts every token.
fn parse_codex(path: &Path) -> std::io::Result<Days> {
    let (mut modern, mut legacy) = (Days::new(), Days::new());
    let mut model = "unknown".to_string();
    let mut running: Option<[i64; 4]> = None;
    let mut seen = std::collections::HashSet::new();
    for line in lines(path)? {
        if !contains(&line, b"\"model\"") && !contains(&line, b"token") {
            continue;
        }
        let Ok(d) = serde_json::from_slice::<Value>(&line) else { continue };
        let kind = d.get("type").and_then(Value::as_str).unwrap_or("");
        let p = d.get("payload").unwrap_or(&Value::Null);
        if kind == "turn_context" || kind == "session_meta" {
            let m = p.get("model").filter(|m| truthy(m)).or_else(|| p.get("payload").and_then(|q| q.get("model")));
            if let Some(m) = m.and_then(Value::as_str).filter(|m| !m.is_empty()) {
                model = m.to_string();
            }
            continue;
        }
        let ts = d.get("timestamp");
        if kind == "token_usage_record" {
            let u = p.get("usage").unwrap_or(&Value::Null);
            if let Some(rid) = p.get("response_id").and_then(Value::as_str).filter(|r| !r.is_empty())
                && !seen.insert(rid.to_string())
            {
                continue;
            }
            let f = |k| num(u.get(k));
            charge(
                &mut modern,
                local_day(ts),
                &model,
                f("input_tokens"),
                f("cached_input_tokens"),
                f("cache_write_input_tokens"),
                f("output_tokens"),
            );
        } else if kind == "event_msg" && p.get("type").and_then(Value::as_str) == Some("token_count") {
            let Some(info) = p.get("info").filter(|i| truthy(i)) else { continue };
            let tot = info.get("total_token_usage").unwrap_or(&Value::Null);
            let cur = ["input_tokens", "cached_input_tokens", "cache_write_input_tokens", "output_tokens"]
                .map(|k| num(tot.get(k)));
            let delta = match running {
                Some(r) if cur[0] >= r[0] => [cur[0] - r[0], cur[1] - r[1], cur[2] - r[2], cur[3] - r[3]],
                _ => cur, // first record, or the session restarted
            };
            running = Some(cur);
            if delta.iter().any(|&v| v != 0) {
                charge(&mut legacy, local_day(ts), &model, delta[0], delta[1], delta[2], delta[3]);
            }
        }
    }
    Ok(if modern.is_empty() { legacy } else { modern })
}

pub(crate) fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/* ---- collection ---------------------------------------------------------- */

pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home().join(".claude"))
}

pub fn codex_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".codex"))
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Every regular *.jsonl under dir: hidden names and symlinks are skipped.
fn jsonl_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            jsonl_under(&path, out);
        } else if kind.is_file() && name.ends_with(".jsonl") {
            out.push(path);
        }
    }
}

struct Entry {
    stamp: String,
    days: Days,
    claude: Option<Vec<claude::Response>>,
}

/// Re-counts the month on demand, re-reading only transcripts that changed.
pub struct Counter {
    cache: HashMap<PathBuf, Entry>,
    cache_path: PathBuf,
    groups: BTreeMap<Group, Days>,
}

/// This calendar month (local time), Claude and Codex summed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonthUsage {
    pub tokens: i64,
    pub usd: f64,
}

fn month_usage(tally: Tally) -> MonthUsage {
    MonthUsage { tokens: tally.tokens, usd: (tally.cost * 100.0).round() / 100.0 }
}

#[derive(Clone, Debug)]
pub struct GroupUsage {
    pub group: Group,
    pub usage: MonthUsage,
}

#[derive(Clone, Debug)]
pub struct UsageReport {
    pub period: String,
    pub total: MonthUsage,
    pub groups: Vec<GroupUsage>,
}

impl UsageReport {
    pub fn for_surface(&self, surface: Surface) -> MonthUsage {
        let mut tally = Tally::default();
        for group in &self.groups {
            if group.group.surface == surface {
                tally.add(Tally { tokens: group.usage.tokens, cost: group.usage.usd });
            }
        }
        month_usage(tally)
    }

    pub fn json(&self) -> Value {
        let usage = |u: MonthUsage| json!({"tokens": u.tokens, "usd": (u.usd * 100.0).round() / 100.0});
        json!({"period": self.period, "total": usage(self.total),
            "attribution": "saved entrypoint and recorded session owner; not billing identity",
            "surfaces": ([Surface::Desktop, Surface::Cli, Surface::Unassigned, Surface::Codex]
                .map(|s| json!({"surface": s.name(), "usage": usage(self.for_surface(s))}))),
            "groups": self.groups.iter().map(|g| json!({
                "surface":g.group.surface.name(), "account_id":g.group.account_id,
                "organization_id":g.group.organization_id, "usage":usage(g.usage),
            })).collect::<Vec<_>>()})
    }

    pub fn display_rows(&self) -> Vec<crate::scene::UsageRow> {
        let mut rows = Vec::new();
        for (surface, label) in [
            (Surface::Desktop, "CLAUDE DESKTOP"),
            (Surface::Cli, "CLAUDE CLI"),
            (Surface::Unassigned, "CLAUDE UNASSIGNED"),
            (Surface::Codex, "CODEX"),
        ] {
            let usage = self.for_surface(surface);
            rows.push(crate::scene::UsageRow { label: label.into(), tokens: usage.tokens as f64, usd: usage.usd });
        }
        rows
    }

    pub fn account_display_rows(&self) -> Vec<crate::scene::UsageRow> {
        self.groups
            .iter()
            .map(|g| {
                let app = match g.group.surface {
                    Surface::Desktop => "DESKTOP",
                    Surface::Cli => "CLI",
                    Surface::Unassigned => "CLAUDE?",
                    Surface::Codex => "CODEX",
                };
                let identity = g.group.organization_id.as_ref().or(g.group.account_id.as_ref());
                let label = if g.group.surface == Surface::Codex {
                    app.to_owned()
                } else {
                    format!(
                        "{app} {}",
                        identity
                            .map(|s| s.chars().take(8).collect::<String>().to_uppercase())
                            .unwrap_or_else(|| "UNKNOWN".into())
                    )
                };
                crate::scene::UsageRow { label, tokens: g.usage.tokens as f64, usd: g.usage.usd }
            })
            .collect()
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

impl Counter {
    pub fn new() -> Self {
        let dir = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| {
            if cfg!(target_os = "macos") { home().join("Library/Caches") } else { home().join(".cache") }
        });
        let cache_path = dir.join("token-cafe").join("usage.json");
        let cache = Self::load(&cache_path).unwrap_or_default();
        Counter { cache, cache_path, groups: BTreeMap::new() }
    }

    fn load(path: &Path) -> Option<HashMap<PathBuf, Entry>> {
        let v: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        if v.get("version")?.as_u64()? != CACHE_VERSION {
            return None;
        }
        let mut out = HashMap::new();
        for (path, e) in v.get("files")?.as_object()? {
            let mut days = Days::new();
            for (day, t) in e.get("days")?.as_object()? {
                days.insert(day.clone(), Tally { tokens: t.get(0)?.as_i64()?, cost: t.get(1)?.as_f64()? });
            }
            out.insert(
                PathBuf::from(path),
                Entry {
                    stamp: e.get("stamp")?.as_str()?.to_string(),
                    days,
                    claude: match e.get("claude") {
                        Some(Value::Array(rows)) => {
                            Some(rows.iter().map(claude::Response::from_json).collect::<Option<Vec<_>>>()?)
                        }
                        _ => None,
                    },
                },
            );
        }
        Some(out)
    }

    fn save(&self) {
        let files: serde_json::Map<String, Value> = self
            .cache
            .iter()
            .map(|(p, e)| {
                let days: serde_json::Map<String, Value> =
                    e.days.iter().map(|(d, t)| (d.clone(), json!([t.tokens, t.cost]))).collect();
                (p.to_string_lossy().into_owned(), json!({ "stamp": e.stamp, "days": days, "claude": e.claude.as_ref().map(|rows| rows.iter().map(claude::Response::json).collect::<Vec<_>>()) }))
            })
            .collect();
        let doc = json!({ "version": CACHE_VERSION, "files": files });
        let tmp = self.cache_path.with_extension("json.tmp");
        let ok = self.cache_path.parent().is_some_and(|d| fs::create_dir_all(d).is_ok())
            && fs::write(&tmp, doc.to_string()).is_ok();
        if ok {
            let _ = fs::rename(&tmp, &self.cache_path);
        }
    }

    /// Totals per local day over every transcript written since `since`
    /// (epoch seconds): an older file holds nothing newer than its mtime.
    /// Returns the days and how many files were re-read.
    pub fn collect(&mut self, since: f64) -> (Days, usize) {
        self.collect_at(since, &crate::claude_config::roots(), &codex_dir())
    }

    fn collect_at(&mut self, since: f64, roots: &[PathBuf], codex_root: &Path) -> (Days, usize) {
        let (mut claude, mut codex) = (Vec::new(), Vec::new());
        for root in roots {
            jsonl_under(&root.join("projects"), &mut claude);
        }
        // Overlapping roots and aliases must not charge the same file twice.
        let mut seen = std::collections::HashSet::new();
        claude.retain(|path| seen.insert(crate::claude_config::normalize(path.clone())));
        jsonl_under(&codex_root.join("sessions"), &mut codex);
        let mut fresh = HashMap::new();
        let mut totals = Days::new();
        let mut reparsed = 0;
        let mut responses = HashMap::new();
        self.groups.clear();
        for (files, is_claude) in [(claude, true), (codex, false)] {
            for path in files {
                let Ok(st) = fs::metadata(&path) else { continue };
                let mtime = st.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
                let Some(mtime) = mtime else { continue };
                if mtime.as_secs_f64() < since {
                    continue;
                }
                let stamp = format!("{}:{}", st.len(), mtime.as_nanos());
                let entry = match self.cache.remove(&path) {
                    Some(e) if e.stamp == stamp => e,
                    _ => {
                        reparsed += 1;
                        // Retry transient read failures on the next poll instead of caching zero.
                        if is_claude {
                            let Ok(rows) = claude::parse(&path) else { continue };
                            Entry { stamp, days: Days::new(), claude: Some(rows) }
                        } else {
                            let Ok(days) = parse_codex(&path) else { continue };
                            Entry { stamp, days, claude: None }
                        }
                    }
                };
                if let Some(rows) = &entry.claude {
                    for row in rows {
                        claude::merge(&mut responses, row.clone());
                    }
                } else {
                    let group = self.groups.entry(Group::unknown(Surface::Codex)).or_default();
                    for (d, t) in &entry.days {
                        totals.entry(d.clone()).or_default().add(*t);
                        group.entry(d.clone()).or_default().add(*t);
                    }
                }
                fresh.insert(path, entry);
            }
        }
        for row in responses.into_values() {
            totals.entry(row.day.clone()).or_default().add(row.tally);
            self.groups.entry(row.group).or_default().entry(row.day).or_default().add(row.tally);
        }
        let dropped = !self.cache.is_empty();
        self.cache = fresh;
        if reparsed > 0 || dropped {
            self.save();
        }
        (totals, reparsed)
    }

    /// This calendar month (local time), Claude and Codex summed.
    pub fn month(&mut self) -> MonthUsage {
        self.month_report().total
    }

    pub fn month_report(&mut self) -> UsageReport {
        let today = Local::now().date_naive();
        let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
        let since = Local
            .from_local_datetime(&first.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .map_or(0.0, |t| t.timestamp() as f64);
        let (days, _) = self.collect(since);
        let (lo, hi) = (first.format("%Y-%m-%d").to_string(), today.format("%Y-%m-%d").to_string());
        let mut sum = Tally::default();
        for (_, t) in days.range(lo.clone()..=hi.clone()) {
            sum.add(*t);
        }
        let mut groups = Vec::new();
        for (group, days) in &self.groups {
            let mut tally = Tally::default();
            for (_, value) in days.range(lo.clone()..=hi.clone()) {
                tally.add(*value);
            }
            if tally.tokens != 0 || tally.cost != 0.0 {
                groups.push(GroupUsage {
                    group: group.clone(),
                    usage: MonthUsage { tokens: tally.tokens, usd: tally.cost },
                });
            }
        }
        UsageReport { period: today.format("%Y-%m").to_string(), total: month_usage(sum), groups }
    }
}

/// 1.6 B / 53.0 M / 999.0 K, for the log.
pub fn human(n: i64) -> String {
    for (unit, div) in [("B", 1_000_000_000), ("M", 1_000_000), ("K", 1_000)] {
        if n >= div {
            return format!("{:.1} {unit}", n as f64 / div as f64);
        }
    }
    n.to_string()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn usage_counts_all_claude_roots_without_charging_aliases_twice() {
        let dir = crate::collect::TestDir::new();
        let a = dir.0.join("personal");
        let b = dir.0.join("work");
        for (root, count) in [(&a, 10), (&b, 20)] {
            fs::create_dir_all(root.join("projects/moon")).unwrap();
            let record = json!({"type":"assistant","timestamp":"2026-10-10T12:00:00", "requestId":"r",
                "message":{"id":format!("m-{count}"),"model":"claude-sonnet-5","usage":{"input_tokens":count,"output_tokens":1}}});
            fs::write(root.join("projects/moon/session.jsonl"), record.to_string()).unwrap();
        }
        let alias = dir.0.join("alias");
        std::os::unix::fs::symlink(&a, &alias).unwrap();
        let mut counter =
            Counter { cache: HashMap::new(), cache_path: dir.0.join("usage.json"), groups: BTreeMap::new() };
        let roots = vec![a.clone(), b.clone(), a, alias];
        let (days, parsed) = counter.collect_at(0., &roots, &dir.0.join("no-codex"));
        assert_eq!(parsed, 2);
        assert_eq!(days["2026-10-10"].tokens, 32);
        let (again, parsed) = counter.collect_at(0., &roots, &dir.0.join("no-codex"));
        assert_eq!(parsed, 0);
        assert_eq!(again["2026-10-10"].tokens, 32);
    }

    #[test]
    fn breakdown_reconciles_and_survives_cache_reload_and_fork_copies() {
        let dir = crate::collect::TestDir::new();
        let root = dir.0.join("claude");
        let project = root.join("projects/moon");
        fs::create_dir_all(&project).unwrap();
        let response = |id, surface, count| {
            json!({"type":"assistant", "entrypoint":surface,
            "timestamp":"2026-10-10T12:00:00", "sessionId":"s",
            "message":{"id":id,"model":"claude-sonnet-5","usage":{"input_tokens":count,"output_tokens":1}}})
        };
        let data = [
            response("one", Some("claude-desktop"), 10),
            response("two", Some("cli"), 20),
            response("three", None, 30),
        ]
        .iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>();
        fs::write(project.join("s.jsonl"), &data).unwrap();
        fs::write(project.join("fork.jsonl"), data).unwrap();
        let cache_path = dir.0.join("cache.json");
        let mut counter = Counter { cache: HashMap::new(), cache_path: cache_path.clone(), groups: BTreeMap::new() };
        let roots = [root];
        let (days, parsed) = counter.collect_at(0., &roots, &dir.0.join("no-codex"));
        assert_eq!(parsed, 2);
        assert_eq!(days["2026-10-10"].tokens, 63);
        assert_eq!(counter.groups.values().flat_map(|days| days.values()).map(|t| t.tokens).sum::<i64>(), 63);
        let surface_tokens = |surface| {
            counter
                .groups
                .iter()
                .filter(|(g, _)| g.surface == surface)
                .flat_map(|(_, days)| days.values())
                .map(|t| t.tokens)
                .sum::<i64>()
        };
        assert_eq!(surface_tokens(Surface::Desktop), 11);
        assert_eq!(surface_tokens(Surface::Cli), 21);
        assert_eq!(surface_tokens(Surface::Unassigned), 31);
        let mut warm = Counter { cache: Counter::load(&cache_path).unwrap(), cache_path, groups: BTreeMap::new() };
        let (days, parsed) = warm.collect_at(0., &roots, &dir.0.join("no-codex"));
        assert_eq!(parsed, 0);
        assert_eq!(days["2026-10-10"].tokens, 63);
        assert_eq!(warm.groups, counter.groups);
    }

    #[test]
    fn history_scan_does_not_follow_cycles_or_duplicate_symlinks() {
        let root = crate::collect::TestDir::new();
        let sub = root.0.join("project");
        fs::create_dir(&sub).unwrap();
        let transcript = sub.join("session.jsonl");
        fs::write(&transcript, b"{}\n").unwrap();
        std::os::unix::fs::symlink(&root.0, sub.join("loop")).unwrap();
        std::os::unix::fs::symlink(&transcript, sub.join("duplicate.jsonl")).unwrap();
        let mut files = Vec::new();
        jsonl_under(&root.0, &mut files);
        assert_eq!(files, vec![transcript]);
    }
}
