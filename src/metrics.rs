//! Application metrics.
//!
//! Until this existed the service emitted traces and logs but no metrics at
//! all — `init_tracing` built a tracer provider and a logger provider and no
//! meter provider — so every question about throughput had to be answered
//! from the outside. A k6 load test could say how fast the *client* was
//! served; nothing could say how many webhooks the service accepted, how many
//! it quietly dropped, or how many calls to Notion a burst actually cost.
//!
//! The instruments here are chosen to answer questions that were asked and
//! could only be measured indirectly:
//!
//! - `webhook_received_total{event_type,outcome}` — 2.9% of production page
//!   events arrive with no `data.parent.data_source_id` and are acknowledged
//!   and then discarded. That was found by reading 2500 log lines out of
//!   Loki; with this it is a line on a graph.
//! - `notion_requests_total{operation,outcome}` — a measured burst produced
//!   roughly one Notion call per webhook (194 webhooks → 194 calls on a single
//!   page). This is that ratio, server-side and continuous, against a limit of
//!   about 3 requests/second.
//! - `webhook_inflight` — work accepted but not finished. A SIGKILL loses
//!   exactly this much: 20 of 20 in a controlled experiment. The gauge makes
//!   the exposure visible before something kills the pod rather than after.
//!   It reads 0 almost always, and that is correct rather than broken: guard
//!   lifetimes measured from the logs have a median of 0.9ms and a peak
//!   concurrency of 1, so there is genuinely nothing in flight at a 15-second
//!   sampling instant. A long investigation treated this as a bug; it was
//!   not. What was broken was the histogram below, whose constant 2.5 made
//!   the upstream look slow when it was not.
//! - `sync_cycle_duration_seconds` / `sync_calendars_total{result}` — the
//!   periodic pull, which holds a database transaction open for its whole
//!   duration.
//!
//! One mapping to keep in mind when querying these: OpenTelemetry puts the
//! resource's `service.name` onto Prometheus's `job` label. There is no
//! `service_name` label on the individual series — that only appears on
//! `target_info` — so filtering a dashboard by environment means
//! `job="notion-caldav-saas-stg"`, not `service_name=`.
//!
//! Instruments are created once behind a `OnceLock` rather than threaded
//! through `AppState`: they are needed in `webhook.rs` and in several places
//! in `caldav.rs`, and passing a `Meter` to each would be a much larger diff
//! than the measurement justifies. `global::meter` returns a no-op meter when
//! no provider is installed, so this is also safe when OTLP is unconfigured
//! (local development) — the calls become cheap no-ops rather than needing a
//! feature flag.

use opentelemetry::metrics::{Counter, Histogram};
use opentelemetry::{global, KeyValue};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::OnceLock;

/// Backing store for the in-flight gauge.
///
/// An UpDownCounter was the obvious choice and did not work: under a six
/// minute ramp to 60 webhooks/second with 1.5s of upstream latency -- roughly
/// 85 tasks in flight at any moment -- it exported zero on every single
/// collection. Rather than keep guessing at temporality semantics, this is an
/// ObservableGauge reading an AtomicI64, which is unambiguous: whatever the
/// counter says at collection time is what gets exported.
static INFLIGHT: AtomicI64 = AtomicI64::new(0);

pub struct Instruments {
    webhook_received: Counter<u64>,
    notion_requests: Counter<u64>,
    notion_duration: Histogram<f64>,
    sync_cycle_duration: Histogram<f64>,
    sync_calendars: Counter<u64>,
    caldav_requests: Counter<u64>,
}

static INSTRUMENTS: OnceLock<Instruments> = OnceLock::new();

fn instruments() -> &'static Instruments {
    INSTRUMENTS.get_or_init(|| {
        let meter = global::meter("notion-ical-sync");
        // Observed, not pushed: the callback runs at collection time and
        // reports the instantaneous depth.
        let _ = meter
            .i64_observable_gauge("notioncal.webhook.inflight")
            .with_description(
                "Webhooks acknowledged but not yet finished processing. Everything counted here \
                 is lost if the process dies.",
            )
            .with_callback(|observer| observer.observe(INFLIGHT.load(Ordering::Relaxed), &[]))
            .build();
        Instruments {
            webhook_received: meter
                .u64_counter("notioncal.webhook.received")
                .with_description(
                    "Notion webhooks received, by event type and what the service did with it",
                )
                .build(),
            notion_requests: meter
                .u64_counter("notioncal.notion.requests")
                .with_description("Requests made to the Notion API, by operation and outcome")
                .build(),
            notion_duration: meter
                .f64_histogram("notioncal.notion.request.duration")
                .with_unit("s")
                .with_description("Latency of Notion API calls")
                // Explicit boundaries, because the OTel defaults are
                // [0, 5, 10, 25, 50, 75, 100, 250, 500, ...] and are meant for
                // MILLISECONDS. Recording seconds against them puts every real
                // observation into the single (0, 5] bucket, and
                // histogram_quantile then linearly interpolates to exactly
                // 2.5 — forever, for any input. That constant was read as
                // "upstream latency is 2.5s, the injection is working" and
                // used to validate several load tests. It measured nothing.
                // The giveaway was that it was 2.5000 every single time.
                .with_boundaries(vec![
                    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
                ])
                .build(),
            sync_cycle_duration: meter
                .f64_histogram("notioncal.sync.cycle.duration")
                .with_unit("s")
                // Same reason as above, shifted up: a pull cycle on production
                // runs 130-200s.
                .with_boundaries(vec![
                    0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 180.0, 300.0, 600.0,
                ])
                .with_description(
                    "Duration of a full refresh_all cycle. A database transaction is held open \
                     for all of it.",
                )
                .build(),
            sync_calendars: meter
                .u64_counter("notioncal.sync.calendars")
                .with_description("Calendars processed per pull cycle, by result")
                .build(),
            caldav_requests: meter
                .u64_counter("notioncal.caldav.requests")
                .with_description("CalDAV requests served, by method and status class")
                .build(),
        }
    })
}

/// What `handle_notion_webhook` decided to do with an event.
///
/// `DroppedNoDataSource` is the one to watch: the event was verified,
/// acknowledged with 200, and then discarded because it carried no
/// `data.parent.data_source_id`. Notion will not send it again.
pub enum WebhookOutcome {
    HandledPage,
    HandledDataSource,
    DroppedNoDataSource,
    DroppedUnverified,
    DroppedInvalidJson,
    Handshake,
    NoSecretConfigured,
}

impl WebhookOutcome {
    fn as_str(&self) -> &'static str {
        match self {
            Self::HandledPage => "handled_page",
            Self::HandledDataSource => "handled_data_source",
            Self::DroppedNoDataSource => "dropped_no_data_source",
            Self::DroppedUnverified => "dropped_unverified",
            Self::DroppedInvalidJson => "dropped_invalid_json",
            Self::Handshake => "handshake",
            Self::NoSecretConfigured => "no_secret_configured",
        }
    }
}

pub fn webhook_received(event_type: &str, outcome: WebhookOutcome) {
    instruments().webhook_received.add(
        1,
        &[
            KeyValue::new("event_type", event_type.to_string()),
            KeyValue::new("outcome", outcome.as_str()),
        ],
    );
}

/// Guard that keeps `webhook.inflight` honest even if the work panics.
///
/// Decrementing at the end of the happy path would leak the counter upward on
/// every panic, and a gauge that only ever rises is worse than no gauge.
pub struct InflightGuard(());

impl InflightGuard {
    pub fn new() -> Self {
        // Touch the instruments so the observable gauge is registered even if
        // this is the first metric activity in the process; without it the
        // callback would not exist yet and the first burst would go
        // unobserved.
        let _ = instruments();
        INFLIGHT.fetch_add(1, Ordering::Relaxed);
        Self(())
    }
}

impl Default for InflightGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        INFLIGHT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// One Notion API call. `outcome` is a status class or a transport error, not
/// the exact code, so the series count stays bounded.
pub fn notion_request(operation: &str, outcome: &str, seconds: f64) {
    let labels = [
        KeyValue::new("operation", operation.to_string()),
        KeyValue::new("outcome", outcome.to_string()),
    ];
    instruments().notion_requests.add(1, &labels);
    instruments().notion_duration.record(seconds, &labels);
}

/// Classify a response for the `outcome` label without letting every distinct
/// status code become its own time series.
pub fn status_class(status: u16) -> &'static str {
    match status {
        200..=299 => "2xx",
        401 => "401",
        404 => "404",
        429 => "429",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    }
}

pub fn sync_cycle(seconds: f64, succeeded: u64, failed: u64, skipped: u64) {
    instruments().sync_cycle_duration.record(seconds, &[]);
    for (result, n) in [
        ("succeeded", succeeded),
        ("failed", failed),
        ("skipped", skipped),
    ] {
        if n > 0 {
            instruments()
                .sync_calendars
                .add(n, &[KeyValue::new("result", result)]);
        }
    }
}

pub fn caldav_request(method: &str, status: u16) {
    instruments().caldav_requests.add(
        1,
        &[
            KeyValue::new("method", method.to_string()),
            KeyValue::new("status", status_class(status)),
        ],
    );
}
