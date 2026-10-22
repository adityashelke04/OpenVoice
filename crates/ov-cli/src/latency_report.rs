//! `ov latency`: where dictation time goes, by how long people spoke.
//!
//! Bucketed by audio length because decode time grows with it. A single
//! percentile across a 2-second note and a 40-second paragraph describes
//! neither.

use ov_core::latency::StageTimes;

use crate::stats::percentile;

const BUCKETS: [(&str, u64, u64); 4] = [
    ("<3s", 0, 3_000),
    ("3-7s", 3_000, 7_000),
    ("7-15s", 7_000, 15_000),
    (">15s", 15_000, u64::MAX),
];

/// p50 and p90 of one stage within one bucket.
pub struct Cell {
    /// Stage name as printed.
    pub stage: &'static str,
    /// Median.
    pub p50: Option<u64>,
    /// 90th percentile: the slow sessions the user remembers.
    pub p90: Option<u64>,
}

/// One audio-length bucket.
pub struct Row {
    /// Bucket label.
    pub label: &'static str,
    /// Sessions in the bucket.
    pub n: usize,
    /// Sessions whose tail was already decoded at release.
    pub reused: usize,
    /// One cell per stage, in pipeline order.
    pub cells: Vec<Cell>,
}

type Stage = fn(&StageTimes) -> Option<u64>;

const STAGES: [(&str, Stage); 7] = [
    ("latency", |t| Some(t.latency_ms)),
    ("lag", |t| {
        t.total_ms.map(|total| t.latency_ms.saturating_sub(total))
    }),
    ("stop", |t| t.stop_ms),
    ("queue", |t| t.queue_ms),
    ("decode", |t| t.decode_ms),
    ("format", |t| t.format_ms),
    ("inject", |t| t.inject_ms),
];

/// Summarise parsed latency lines into one row per bucket.
#[must_use]
pub fn summarize(all: &[StageTimes]) -> Vec<Row> {
    BUCKETS
        .iter()
        .map(|&(label, lo, hi)| {
            let rows: Vec<&StageTimes> = all
                .iter()
                .filter(|t| (lo..hi).contains(&t.audio_ms))
                .collect();
            let cells = STAGES
                .iter()
                .map(|&(stage, pick)| {
                    let values: Vec<u64> = rows.iter().filter_map(|t| pick(t)).collect();
                    Cell {
                        stage,
                        p50: percentile(&values, 50.0),
                        p90: percentile(&values, 90.0),
                    }
                })
                .collect();
            Row {
                label,
                n: rows.len(),
                reused: rows.iter().filter(|t| t.facts.reused).count(),
                cells,
            }
        })
        .collect()
}

/// Reject partial/invalid timings rather than labeling decoder-only data end-to-end.
pub fn validate_end_to_end(all: &[StageTimes]) -> Result<(), String> {
    if all.is_empty() {
        return Err("no end-to-end sessions measured".into());
    }
    let mut sessions = std::collections::HashSet::new();
    for t in all {
        if !sessions.insert(t.session.0) {
            return Err("duplicate session IDs; use one app run per gate".into());
        }
        let stages = [t.stop_ms, t.queue_ms, t.decode_ms, t.format_ms, t.inject_ms];
        let Some(sum) = stages
            .into_iter()
            .try_fold(0u64, |sum, stage| sum.checked_add(stage?))
        else {
            return Err(format!(
                "session {} lacks capture-stop/queue/decode/format/inject timings",
                t.session.0
            ));
        };
        if t.total_ms != Some(sum) || t.latency_ms < sum || t.audio_ms == 0 {
            return Err(format!(
                "session {} has inconsistent end-to-end timings",
                t.session.0
            ));
        }
    }
    Ok(())
}

/// Validate live app log rows including successful delivery evidence.
pub fn gate_log(text: &str) -> Result<Vec<StageTimes>, String> {
    let mut all = Vec::new();
    let mut failed = 0;
    for line in text
        .lines()
        .filter(|line| line.contains(ov_core::latency::LINE_MARKER))
    {
        let times = StageTimes::parse(line).ok_or("malformed app latency record")?;
        let outcome = line
            .split_whitespace()
            .find_map(|part| part.strip_prefix("outcome="));
        if outcome != Some("delivered") {
            failed += 1;
        }
        all.push(times);
    }
    if failed > 0 {
        return Err(format!(
            "Gate C has {failed}/{} delivery failures or missing outcomes",
            all.len()
        ));
    }
    gate_c(&all)?;
    Ok(all)
}

/// Gate C: thirty real app sessions, every length bucket represented, bounded p50/p90.
pub fn gate_c(all: &[StageTimes]) -> Result<(), String> {
    validate_end_to_end(all)?;
    if all.len() < 30 {
        return Err(format!("Gate C needs 30 sessions, found {}", all.len()));
    }
    for row in summarize(all) {
        if row.n < 5 {
            return Err(format!(
                "Gate C bucket {} needs at least 5 sessions, found {}",
                row.label, row.n
            ));
        }
        let latency = &row.cells[0];
        if latency.p50.unwrap_or(u64::MAX) > 250 || latency.p90.unwrap_or(u64::MAX) > 600 {
            return Err(format!(
                "Gate C bucket {} misses p50<=250/p90<=600 ms",
                row.label
            ));
        }
    }
    Ok(())
}

/// A fixed-width table: `p50/p90` in milliseconds per stage.
#[must_use]
pub fn render(rows: &[Row]) -> String {
    fn pair(c: &Cell) -> String {
        let f = |v: Option<u64>| v.map_or_else(|| "-".to_owned(), |v| v.to_string());
        format!("{}/{}", f(c.p50), f(c.p90))
    }
    let mut out = format!("{:<6} {:>4} {:>7}", "audio", "n", "reused");
    // Named from the cells themselves, so a column header can never sit above
    // another stage's numbers.
    for c in rows.first().map_or(&[][..], |r| &r.cells[..]) {
        out.push_str(&format!(" {:>12}", c.stage));
    }
    out.push('\n');
    for row in rows {
        out.push_str(&format!("{:<6} {:>4} {:>7}", row.label, row.n, row.reused));
        for c in &row.cells {
            out.push_str(&format!(" {:>12}", pair(c)));
        }
        out.push('\n');
    }
    out.push_str("\ncells are p50/p90 in ms. lag = release waited in the session loop.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ov_core::latency::DecodeFacts;
    use ov_core::types::SessionId;

    fn times(audio_ms: u64, latency_ms: u64, decode_ms: Option<u64>, reused: bool) -> StageTimes {
        StageTimes {
            session: SessionId(1),
            audio_ms,
            latency_ms,
            stop_ms: Some(10),
            queue_ms: Some(0),
            decode_ms,
            format_ms: Some(1),
            inject_ms: Some(30),
            total_ms: Some(latency_ms.saturating_sub(5)),
            facts: DecodeFacts {
                reused,
                segments: 1,
                fallback: false,
            },
        }
    }

    fn cell<'a>(row: &'a Row, stage: &str) -> &'a Cell {
        row.cells
            .iter()
            .find(|c| c.stage == stage)
            .expect("stage column")
    }

    #[test]
    fn end_to_end_report_rejects_missing_pipeline_stages() {
        let mut t = times(1_000, 100, Some(20), false);
        t.stop_ms = None;
        assert!(validate_end_to_end(&[t]).is_err());
    }

    #[test]
    fn gate_c_requires_thirty_sessions_and_every_bucket() {
        assert!(gate_c(&[]).is_err());
        let all = vec![times(1_000, 100, Some(20), false); 30];
        assert!(gate_c(&all).is_err());
    }

    #[test]
    fn complete_gate_accepts_all_buckets_and_rejects_slow_or_partial_runs() {
        let mut all: Vec<_> = (0..32)
            .map(|i| {
                let mut t = times([1_000, 4_000, 10_000, 20_000][i % 4], 100, Some(20), false);
                t.session = SessionId(i as u64 + 1);
                t.total_ms = Some(61);
                t
            })
            .collect();
        assert!(gate_c(&all).is_ok());
        all[0].format_ms = None;
        assert!(gate_c(&all).is_err());
        all[0].format_ms = Some(1);
        for t in &mut all {
            t.latency_ms = 601;
        }
        assert!(gate_c(&all).unwrap_err().contains("misses"));
    }

    #[test]
    fn gate_log_rejects_missing_delivery_evidence_and_malformed_records() {
        let t = times(1_000, 100, Some(20), false);
        assert!(gate_log(&t.to_line())
            .unwrap_err()
            .contains("missing outcomes"));
        assert!(gate_log("latency session=invalid outcome=delivered")
            .unwrap_err()
            .contains("malformed"));
    }

    #[test]
    fn sessions_land_in_the_bucket_for_their_length() {
        let rows = summarize(&[
            times(2_999, 300, Some(200), false),
            times(3_000, 400, Some(300), false),
            times(15_000, 900, Some(800), false),
        ]);
        assert_eq!(
            rows.iter().map(|r| r.n).collect::<Vec<_>>(),
            vec![1, 1, 0, 1]
        );
    }

    #[test]
    fn a_stage_that_never_happened_does_not_count_as_zero() {
        let rows = summarize(&[
            times(1_000, 300, None, false),
            times(1_000, 500, Some(200), false),
        ]);
        assert_eq!(cell(&rows[0], "decode").p50, Some(200));
    }

    #[test]
    fn lag_is_how_long_the_release_waited_before_the_engine_acted() {
        let rows = summarize(&[times(1_000, 300, Some(200), false)]);
        assert_eq!(cell(&rows[0], "lag").p50, Some(5));
    }

    #[test]
    fn reused_sessions_are_counted() {
        let rows = summarize(&[
            times(1_000, 90, Some(1), true),
            times(1_000, 300, Some(200), false),
        ]);
        assert_eq!(rows[0].reused, 1);
    }

    #[test]
    fn the_table_names_every_bucket() {
        let out = render(&summarize(&[times(1_000, 300, Some(200), false)]));
        for label in ["<3s", "3-7s", "7-15s", ">15s"] {
            assert!(out.contains(label), "{out}");
        }
    }
}
