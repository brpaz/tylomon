use std::collections::HashMap;

use crate::export::Names;
use crate::http::types::HttpExchange;

/// Aggregated traffic from one client to one server endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeStats {
    /// Container name, or IP when unresolved.
    pub src: String,
    pub dst: String,
    pub dst_port: u16,
    pub requests: usize,
    /// Responses with a 4xx status.
    pub client_errors: usize,
    /// Responses with a 5xx status, plus requests that never got a response.
    pub failures: usize,
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
}

impl EdgeStats {
    /// Share of requests that failed, in percent.
    pub fn failure_rate(&self) -> f64 {
        if self.requests == 0 {
            return 0.0;
        }
        self.failures as f64 * 100.0 / self.requests as f64
    }
}

#[derive(Default)]
struct Accumulator {
    requests: usize,
    client_errors: usize,
    failures: usize,
    durations: Vec<u64>,
}

/// Nearest-rank percentile of an ascending-sorted slice.
fn percentile(sorted: &[u64], pct: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (pct / 100.0 * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

/// Group exchanges by `(source, destination, destination port)`, busiest first.
pub fn service_map<'a>(
    exchanges: impl IntoIterator<Item = &'a HttpExchange>,
    names: &Names,
) -> Vec<EdgeStats> {
    let label = |ip: &str| names.get(ip).cloned().unwrap_or_else(|| ip.to_string());

    let mut groups: HashMap<(String, String, u16), Accumulator> = HashMap::new();
    for ex in exchanges {
        let key = (label(&ex.src_ip), label(&ex.dst_ip), ex.dst_port);
        let acc = groups.entry(key).or_default();
        acc.requests += 1;
        match ex.response.as_ref().map(|r| r.status_code) {
            Some(400..=499) => acc.client_errors += 1,
            Some(500..) | None => acc.failures += 1,
            Some(_) => {}
        }
        acc.durations.extend(ex.duration_ms);
    }

    let mut edges: Vec<EdgeStats> = groups
        .into_iter()
        .map(|((src, dst, dst_port), mut acc)| {
            acc.durations.sort_unstable();
            EdgeStats {
                src,
                dst,
                dst_port,
                requests: acc.requests,
                client_errors: acc.client_errors,
                failures: acc.failures,
                p50_ms: percentile(&acc.durations, 50.0),
                p95_ms: percentile(&acc.durations, 95.0),
            }
        })
        .collect();
    edges.sort_by(|a, b| {
        b.requests
            .cmp(&a.requests)
            .then_with(|| (&a.src, &a.dst, a.dst_port).cmp(&(&b.src, &b.dst, b.dst_port)))
    });
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpRequest, HttpResponse};

    fn exchange(src: &str, dst: &str, status: Option<u16>, ms: Option<u64>) -> HttpExchange {
        HttpExchange {
            request: HttpRequest {
                method: "GET".into(),
                path: "/".into(),
                version: "HTTP/1.1".into(),
                headers: vec![],
                body: None,
            },
            response: status.map(|code| HttpResponse {
                status_code: code,
                reason: String::new(),
                version: "HTTP/1.1".into(),
                headers: vec![],
                body: None,
            }),
            src_ip: src.into(),
            dst_ip: dst.into(),
            src_port: 40000,
            dst_port: 80,
            id: 0,
            protocol: Default::default(),
            timestamp_us: None,
            duration_ms: ms,
        }
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let data = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(percentile(&data, 50.0), Some(50));
        assert_eq!(percentile(&data, 95.0), Some(100));
        assert_eq!(percentile(&[7], 50.0), Some(7));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn groups_by_edge_and_counts_outcomes() {
        let exchanges = [
            exchange("a", "b", Some(200), Some(10)),
            exchange("a", "b", Some(404), Some(20)),
            exchange("a", "b", Some(503), Some(30)),
            exchange("a", "b", None, None),
            exchange("a", "c", Some(200), Some(5)),
        ];
        let edges = service_map(&exchanges, &Names::new());

        assert_eq!(edges.len(), 2);
        let ab = &edges[0];
        assert_eq!((ab.src.as_str(), ab.dst.as_str()), ("a", "b"));
        assert_eq!(ab.requests, 4);
        assert_eq!(ab.client_errors, 1);
        assert_eq!(ab.failures, 2);
        assert_eq!(ab.failure_rate(), 50.0);
        assert_eq!(ab.p50_ms, Some(20));
        assert_eq!(ab.p95_ms, Some(30));
        assert_eq!(edges[1].requests, 1);
    }

    #[test]
    fn edges_use_container_names_and_merge_ips() {
        let names = Names::from([("10.0.0.2".into(), "poller".into())]);
        let exchanges = [
            exchange("10.0.0.2", "10.0.0.3", Some(200), Some(1)),
            exchange("10.0.0.2", "10.0.0.3", Some(200), Some(1)),
        ];
        let edges = service_map(&exchanges, &names);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].src, "poller");
        assert_eq!(edges[0].dst, "10.0.0.3");
    }

    #[test]
    fn different_ports_are_separate_edges() {
        let mut other = exchange("a", "b", Some(200), Some(1));
        other.dst_port = 8080;
        let exchanges = [exchange("a", "b", Some(200), Some(1)), other];
        assert_eq!(service_map(&exchanges, &Names::new()).len(), 2);
    }

    #[test]
    fn edge_without_durations_has_no_percentiles() {
        let edges = service_map(&[exchange("a", "b", None, None)], &Names::new());
        assert_eq!(edges[0].p50_ms, None);
        assert_eq!(edges[0].p95_ms, None);
    }
}
