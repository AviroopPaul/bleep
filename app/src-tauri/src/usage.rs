//! The Usage tab (0.5.0, F4): model calls, tokens, cost, tools and agent asks over a range of
//! days, bucketed in local time. Everything comes from the local database.
use crate::store::usage::{ApiRow, LlmRow, ToolStat};
use crate::AppState;
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DayRow {
    pub day: String,
    pub calls: i64,
    /// Tokens (in plus out) by purpose. Calls with no token count add nothing here.
    pub tokens: BTreeMap<String, i64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ModelRow {
    pub runtime: String,
    pub model: String,
    pub calls: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost: Option<f64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ToolRow {
    pub tool: String,
    pub calls: i64,
    pub failed: i64,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct AgentStats {
    pub auto: i64,
    pub approved: i64,
    pub denied: i64,
    pub answered: i64,
    pub expired: i64,
    pub queued: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UsageView {
    pub days: u32,
    pub calls: i64,
    pub failed: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    /// US dollars, API key calls only. None when no API call ran in the range.
    pub cost: Option<f64>,
    /// API calls with no known price, so `cost` is low.
    pub unpriced: i64,
    /// Model calls by weekday (Monday first) and local hour: 7 rows of 24.
    pub heat: Vec<Vec<i64>>,
    pub daily: Vec<DayRow>,
    /// Purposes by total tokens, largest first.
    pub purposes: Vec<String>,
    pub models: Vec<ModelRow>,
    pub tools: Vec<ToolRow>,
    pub agents: AgentStats,
}

/// The start of the range: local midnight, `days - 1` days ago.
pub fn range_start<Tz: TimeZone>(now: DateTime<Utc>, days: u32, tz: &Tz) -> DateTime<Utc> {
    let first = now.with_timezone(tz).date_naive() - Duration::days(days.max(1) as i64 - 1);
    tz.from_local_datetime(&first.and_hms_opt(0, 0, 0).unwrap()).earliest().map(|t| t.with_timezone(&Utc)).unwrap_or(now)
}

pub struct Inputs {
    pub llm: Vec<LlmRow>,
    pub api: Vec<ApiRow>,
    pub tools: Vec<ToolStat>,
    pub asks: Vec<(String, i64)>,
}

/// `model_of(runtime, tier)` names the model behind a tier for calls that did not record one.
pub fn summarize<Tz: TimeZone>(inp: Inputs, now: DateTime<Utc>, days: u32, tz: &Tz, model_of: &dyn Fn(&str, &str) -> String) -> UsageView {
    let days = days.clamp(1, 90);
    let first: NaiveDate = range_start(now, days, tz).with_timezone(tz).date_naive();
    let mut daily: Vec<DayRow> = (0..days as i64).map(|i| DayRow { day: (first + Duration::days(i)).to_string(), calls: 0, tokens: BTreeMap::new() }).collect();
    let day_of = |t: DateTime<Utc>| -> Option<usize> {
        let i = (t.with_timezone(tz).date_naive() - first).num_days();
        (0..days as i64).contains(&i).then_some(i as usize)
    };
    let mut heat = vec![vec![0i64; 24]; 7];
    let (mut calls, mut failed, mut tin, mut tout) = (0, 0, 0, 0);
    let mut models: BTreeMap<(String, String), ModelRow> = BTreeMap::new();
    let mut totals: BTreeMap<String, i64> = BTreeMap::new();

    for r in &inp.llm {
        let Some(i) = day_of(r.at) else { continue };
        calls += 1;
        failed += (!r.ok) as i64;
        let local = r.at.with_timezone(tz);
        heat[local.weekday().num_days_from_monday() as usize][local.hour() as usize] += 1;
        daily[i].calls += 1;
        // API key calls are counted in tokens and by model from `api_usage`, which is exact.
        if r.runtime == "api" {
            continue;
        }
        let (a, b) = (r.tokens_in.unwrap_or(0), r.tokens_out.unwrap_or(0));
        tin += a;
        tout += b;
        if a + b > 0 {
            *daily[i].tokens.entry(r.purpose.clone()).or_default() += a + b;
            *totals.entry(r.purpose.clone()).or_default() += a + b;
        }
        let model = model_of(&r.runtime, &r.tier);
        let m = models.entry((r.runtime.clone(), model.clone())).or_insert(ModelRow { runtime: r.runtime.clone(), model, calls: 0, tokens_in: 0, tokens_out: 0, cost: None });
        m.calls += 1;
        m.tokens_in += a;
        m.tokens_out += b;
    }

    let (mut cost, mut unpriced, mut any_api) = (0.0, 0, false);
    for r in &inp.api {
        let Some(i) = day_of(r.at) else { continue };
        any_api = true;
        tin += r.tokens_in;
        tout += r.tokens_out;
        let t = r.tokens_in + r.tokens_out;
        if t > 0 {
            *daily[i].tokens.entry(r.purpose.clone()).or_default() += t;
            *totals.entry(r.purpose.clone()).or_default() += t;
        }
        match r.cost {
            Some(c) => cost += c,
            None => unpriced += 1,
        }
        let m = models.entry(("api".into(), r.model.clone())).or_insert(ModelRow { runtime: "api".into(), model: r.model.clone(), calls: 0, tokens_in: 0, tokens_out: 0, cost: None });
        m.calls += 1;
        m.tokens_in += r.tokens_in;
        m.tokens_out += r.tokens_out;
        if let Some(c) = r.cost {
            m.cost = Some(m.cost.unwrap_or(0.0) + c);
        }
    }

    let mut purposes: Vec<(String, i64)> = totals.into_iter().collect();
    purposes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut models: Vec<ModelRow> = models.into_values().collect();
    models.sort_by(|a, b| b.calls.cmp(&a.calls).then(a.model.cmp(&b.model)));
    let mut agents = AgentStats::default();
    for (d, n) in inp.asks {
        match d.as_str() {
            "auto" => agents.auto += n,
            "approved" => agents.approved += n,
            "denied" => agents.denied += n,
            "answered" => agents.answered += n,
            "expired" => agents.expired += n,
            "queued" => agents.queued += n,
            _ => {}
        }
    }
    UsageView {
        days,
        calls,
        failed,
        tokens_in: tin,
        tokens_out: tout,
        cost: any_api.then_some(cost),
        unpriced,
        heat,
        daily,
        purposes: purposes.into_iter().map(|p| p.0).collect(),
        models,
        tools: inp.tools.into_iter().map(|(tool, calls, failed)| ToolRow { tool, calls, failed }).collect(),
        agents,
    }
}

#[tauri::command]
pub async fn usage_summary(state: tauri::State<'_, AppState>, days: u32) -> Result<UsageView, String> {
    let store = state.store.clone();
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let now = Utc::now();
        let since = range_start(now, days, &chrono::Local);
        let inp = Inputs {
            llm: store.llm_rows_since(since).map_err(|e| e.to_string())?,
            api: store.api_rows_since(since).map_err(|e| e.to_string())?,
            tools: store.tool_stats_since(since, 8).map_err(|e| e.to_string())?,
            asks: store.ask_stats_since(since).map_err(|e| e.to_string())?,
        };
        let model_of = |runtime: &str, tier: &str| -> String {
            match (runtime, tier) {
                ("claude", "main") => cfg.tiers.main.clone(),
                ("claude", "small") => cfg.tiers.small.clone(),
                (_, t) => format!("{t} tier"),
            }
        };
        Ok(summarize(inp, now, days, &chrono::Local, &model_of))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::utc;
    use chrono::FixedOffset;

    fn ist() -> FixedOffset {
        FixedOffset::east_opt(5 * 3600 + 1800).unwrap()
    }

    fn llm(runtime: &str, tier: &str, purpose: &str, tokens: Option<(i64, i64)>, ok: bool, at: &str) -> LlmRow {
        LlmRow { runtime: runtime.into(), tier: tier.into(), purpose: purpose.into(), tokens_in: tokens.map(|t| t.0), tokens_out: tokens.map(|t| t.1), ok, at: utc(at) }
    }

    fn model_of(runtime: &str, tier: &str) -> String {
        format!("{runtime}-{tier}")
    }

    #[test]
    fn range_starts_at_local_midnight() {
        // 17 Nov 2026 10:00 IST. Seven days back from local midnight of 11 Nov = 10 Nov 18:30 UTC.
        assert_eq!(range_start(utc("2026-11-17T04:30:00Z"), 7, &ist()), utc("2026-11-10T18:30:00Z"));
        assert_eq!(range_start(utc("2026-11-17T04:30:00Z"), 1, &ist()), utc("2026-11-16T18:30:00Z"));
    }

    #[test]
    fn buckets_calls_tokens_and_models_in_local_time() {
        let now = utc("2026-11-17T12:00:00Z");
        let inp = Inputs {
            llm: vec![
                // 17 Nov 01:00 IST (Tuesday), still 16 Nov in UTC.
                llm("claude", "main", "chat", Some((1000, 50)), true, "2026-11-16T19:30:00Z"),
                llm("claude", "small", "triage", None, false, "2026-11-17T04:00:00Z"),
                llm("claude", "main", "chat", Some((2000, 100)), true, "2026-11-16T04:00:00Z"),
                // An API key chat: counted as a call, but tokens come from api_usage.
                llm("api", "main", "chat", Some((999, 999)), true, "2026-11-17T05:00:00Z"),
                // Before the range.
                llm("claude", "main", "chat", Some((5, 5)), true, "2026-11-01T00:00:00Z"),
            ],
            api: vec![ApiRow { model: "gpt-x".into(), purpose: "chat".into(), tokens_in: 300, tokens_out: 30, cost: Some(0.25), at: utc("2026-11-17T05:00:00Z") }],
            tools: vec![("calendar_list".into(), 4, 1)],
            asks: vec![("auto".into(), 3), ("denied".into(), 1)],
        };
        let v = summarize(inp, now, 2, &ist(), &model_of);
        assert_eq!((v.days, v.calls, v.failed), (2, 4, 1));
        assert_eq!((v.tokens_in, v.tokens_out), (3300, 180));
        assert_eq!(v.cost, Some(0.25));
        assert_eq!(v.daily.iter().map(|d| d.day.as_str()).collect::<Vec<_>>(), ["2026-11-16", "2026-11-17"]);
        assert_eq!(v.daily[0].tokens.get("chat"), Some(&2100));
        assert_eq!(v.daily[1].tokens.get("chat"), Some(&(1050 + 330)));
        assert_eq!(v.daily[1].calls, 3);
        // Tuesday 01:00 local.
        assert_eq!(v.heat[1][1], 1);
        assert_eq!(v.heat.iter().flatten().sum::<i64>(), 4);
        assert_eq!(v.purposes, ["chat"]);
        let m: Vec<(&str, i64)> = v.models.iter().map(|m| (m.model.as_str(), m.calls)).collect();
        assert_eq!(m, [("claude-main", 2), ("claude-small", 1), ("gpt-x", 1)]);
        assert_eq!(v.models[2].cost, Some(0.25));
        assert_eq!(v.tools[0], ToolRow { tool: "calendar_list".into(), calls: 4, failed: 1 });
        assert_eq!((v.agents.auto, v.agents.denied), (3, 1));
    }

    #[test]
    fn no_api_calls_means_no_cost() {
        let v = summarize(Inputs { llm: vec![], api: vec![], tools: vec![], asks: vec![] }, utc("2026-11-17T12:00:00Z"), 7, &ist(), &model_of);
        assert_eq!((v.cost, v.daily.len(), v.heat.len(), v.heat[0].len()), (None, 7, 7, 24));
    }

    #[test]
    fn store_rows_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let s = crate::store::Store::open(&d.path().join("b.db")).unwrap();
        s.log_llm("claude", "main", "chat", Some(10), Some(2), true).unwrap();
        s.log_tool("bleep_now", "read", true, 3, None).unwrap();
        s.log_tool("bleep_now", "read", false, 3, None).unwrap();
        let since = utc("2020-01-01T00:00:00Z");
        assert_eq!(s.llm_rows_since(since).unwrap()[0].tokens_in, Some(10));
        assert_eq!(s.tool_stats_since(since, 8).unwrap(), vec![("bleep_now".to_string(), 2, 1)]);
        assert!(s.api_rows_since(since).unwrap().is_empty());
        assert!(s.ask_stats_since(since).unwrap().is_empty());
    }
}
