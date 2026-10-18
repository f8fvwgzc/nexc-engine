//! A sliding window of latencies per endpoint for the operator's panel
//! (`GET /admin/latency`): every request and every gRPC call this backend
//! makes lands in a log-spaced histogram of its route for the current
//! minute; the report folds the last five minutes and reads p50, p99 and
//! the max per route from the buckets (a bucket is 20 % wide, so a
//! percentile is read to within that).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use utoipa::ToSchema;

/// Minutes the window covers.
pub const WINDOW_MINUTES: u64 = 5;
/// One slot per minute of the window and one for the minute being overwritten.
const SLOTS: usize = WINDOW_MINUTES as usize + 1;
/// Buckets from [`FIRST_BUCKET_MICROS`] up by [`BUCKET_RATIO`] each: 10 µs to about 330 s.
const BUCKETS: usize = 96;
const FIRST_BUCKET_MICROS: f64 = 10.0;
const BUCKET_RATIO: f64 = 1.2;

/// One route's minute: counts per bucket and the slowest call seen.
#[derive(Debug, Clone)]
struct Series {
    buckets: [u32; BUCKETS],
    count: u64,
    max_micros: u64,
}

impl Series {
    fn new() -> Self {
        Series {
            buckets: [0; BUCKETS],
            count: 0,
            max_micros: 0,
        }
    }

    fn observe(&mut self, micros: u64) {
        self.buckets[bucket_of(micros)] += 1;
        self.count += 1;
        self.max_micros = self.max_micros.max(micros);
    }

    fn merge(&mut self, other: &Series) {
        for (mine, theirs) in self.buckets.iter_mut().zip(&other.buckets) {
            *mine += theirs;
        }
        self.count += other.count;
        self.max_micros = self.max_micros.max(other.max_micros);
    }

    /// The upper bound of the bucket the `p`-th percentile falls in, in ms.
    fn percentile_ms(&self, p: f64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let rank = ((p / 100.0) * self.count as f64).ceil().max(1.0) as u64;
        let mut seen = 0u64;
        for (index, count) in self.buckets.iter().enumerate() {
            seen += u64::from(*count);
            if seen >= rank {
                return (upper_bound_micros(index) / 1000.0).min(self.max_micros as f64 / 1000.0);
            }
        }
        self.max_micros as f64 / 1000.0
    }
}

fn upper_bound_micros(index: usize) -> f64 {
    FIRST_BUCKET_MICROS * BUCKET_RATIO.powi(index as i32)
}

fn bucket_of(micros: u64) -> usize {
    if micros as f64 <= FIRST_BUCKET_MICROS {
        return 0;
    }
    let index = ((micros as f64) / FIRST_BUCKET_MICROS)
        .log(BUCKET_RATIO)
        .ceil();
    (index.max(0.0) as usize).min(BUCKETS - 1)
}

#[derive(Debug, Default)]
struct Slot {
    minute: u64,
    series: BTreeMap<(String, String), Series>,
}

/// The window.
#[derive(Debug)]
pub struct Window {
    slots: Mutex<Vec<Slot>>,
}

impl Default for Window {
    fn default() -> Self {
        Window {
            slots: Mutex::new((0..SLOTS).map(|_| Slot::default()).collect()),
        }
    }
}

/// One endpoint's figures over the window.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct LatencyRow {
    /// `GET`, `POST`, … for HTTP; `gRPC` for the calls this backend makes.
    pub method: String,
    /// The route template (`/api/v1/issues/{iid}`), or `service/method` for gRPC.
    pub route: String,
    pub count: u64,
    pub p50_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

/// What `GET /admin/latency` answers.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LatencyReport {
    /// The window's length.
    pub window_secs: u64,
    /// One row per endpoint seen in the window, slowest p99 first.
    pub rows: Vec<LatencyRow>,
}

fn now_minute() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 60)
}

impl Window {
    /// Records one call of `method` on `route`.
    pub fn observe(&self, method: &str, route: &str, took: Duration) {
        self.observe_at(now_minute(), method, route, took);
    }

    fn observe_at(&self, minute: u64, method: &str, route: &str, took: Duration) {
        let mut slots = self.slots.lock().expect("latency window lock");
        let slot = &mut slots[(minute % SLOTS as u64) as usize];
        if slot.minute != minute {
            slot.minute = minute;
            slot.series.clear();
        }
        slot.series
            .entry((method.to_owned(), route.to_owned()))
            .or_insert_with(Series::new)
            .observe(took.as_micros().min(u64::MAX as u128) as u64);
    }

    /// The last [`WINDOW_MINUTES`] minutes, the current one included.
    pub fn report(&self) -> LatencyReport {
        self.report_at(now_minute())
    }

    fn report_at(&self, minute: u64) -> LatencyReport {
        let oldest = minute.saturating_sub(WINDOW_MINUTES - 1);
        let mut folded: BTreeMap<(String, String), Series> = BTreeMap::new();
        for slot in self.slots.lock().expect("latency window lock").iter() {
            if slot.minute < oldest || slot.minute > minute {
                continue;
            }
            for (key, series) in &slot.series {
                folded
                    .entry(key.clone())
                    .or_insert_with(Series::new)
                    .merge(series);
            }
        }
        let mut rows: Vec<LatencyRow> = folded
            .into_iter()
            .map(|((method, route), series)| LatencyRow {
                method,
                route,
                count: series.count,
                p50_ms: series.percentile_ms(50.0),
                p99_ms: series.percentile_ms(99.0),
                max_ms: series.max_micros as f64 / 1000.0,
            })
            .collect();
        rows.sort_by(|a, b| {
            b.p99_ms
                .total_cmp(&a.p99_ms)
                .then_with(|| a.route.cmp(&b.route))
        });
        LatencyReport {
            window_secs: WINDOW_MINUTES * 60,
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_log_spaced_and_bounded() {
        assert_eq!(bucket_of(0), 0);
        assert_eq!(bucket_of(10), 0);
        assert_eq!(bucket_of(12), 1);
        assert!(bucket_of(1_000) > bucket_of(500));
        assert_eq!(bucket_of(u64::MAX), BUCKETS - 1);
        assert!(
            upper_bound_micros(BUCKETS - 1) > 300e6,
            "the top bucket passes five minutes"
        );
    }

    #[test]
    fn percentiles_are_read_from_the_buckets_within_a_bucket() {
        let w = Window::default();
        for i in 1..=100u64 {
            w.observe_at(1_000, "GET", "/a", Duration::from_millis(i));
        }
        let report = w.report_at(1_000);
        let row = &report.rows[0];
        assert_eq!(
            (row.method.as_str(), row.route.as_str(), row.count),
            ("GET", "/a", 100)
        );
        assert!((45.0..=60.0).contains(&row.p50_ms), "{}", row.p50_ms);
        assert!((95.0..=100.0).contains(&row.p99_ms), "{}", row.p99_ms);
        assert_eq!(row.max_ms, 100.0);
        assert_eq!(report.window_secs, 300);
    }

    #[test]
    fn the_window_slides_and_folds_the_last_five_minutes() {
        let w = Window::default();
        w.observe_at(1_000, "GET", "/old", Duration::from_millis(1));
        w.observe_at(1_003, "GET", "/a", Duration::from_millis(1));
        w.observe_at(1_005, "GET", "/a", Duration::from_millis(9));
        w.observe_at(
            1_005,
            "gRPC",
            "IntelliService/Ask",
            Duration::from_micros(300),
        );
        let report = w.report_at(1_005);
        let routes: Vec<&str> = report.rows.iter().map(|r| r.route.as_str()).collect();
        assert_eq!(
            routes,
            ["/a", "IntelliService/Ask"],
            "slowest first, the old minute gone"
        );
        assert_eq!(report.rows[0].count, 2, "two minutes folded");
        assert_eq!(report.rows[0].max_ms, 9.0);
        // Six minutes on, the slot of minute 1_005 is reused and nothing is left of it.
        w.observe_at(1_011, "GET", "/b", Duration::from_millis(2));
        let later = w.report_at(1_011);
        assert_eq!(later.rows.len(), 1);
        assert_eq!(later.rows[0].route, "/b");
    }
}
