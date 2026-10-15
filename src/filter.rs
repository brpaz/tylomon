use std::collections::HashMap;

use crate::http::types::{HttpExchange, Protocol};

/// A parsed exchange filter: whitespace-separated terms that must all match.
///
/// Term syntax (prefix a term with `!` or `-` to negate it):
/// - `word`: substring of method, path, status, or source/destination name
/// - `src:api`, `dst:gateway`: substring of the container name or IP
/// - `method:post`, `path:/users`: substring match
/// - `status:404`, `status:5xx`, `status:>=400`: exact, class or comparison
/// - `proto:http`, `proto:pg`: protocol (`postgres`, `postgresql` and `sql` also work)
/// - `>500ms`, `<2s`, `>=1s`: response time comparison
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    terms: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Term {
    negate: bool,
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Text(String),
    Src(String),
    Dst(String),
    Method(String),
    Path(String),
    Status(StatusMatch),
    Protocol(Protocol),
    Duration(Cmp, u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

impl Cmp {
    fn apply(self, actual: u64, expected: u64) -> bool {
        match self {
            Cmp::Lt => actual < expected,
            Cmp::Le => actual <= expected,
            Cmp::Gt => actual > expected,
            Cmp::Ge => actual >= expected,
            Cmp::Eq => actual == expected,
        }
    }

    /// Split a leading comparison operator off `s`.
    fn strip(s: &str) -> Option<(Cmp, &str)> {
        [
            (">=", Cmp::Ge),
            ("<=", Cmp::Le),
            (">", Cmp::Gt),
            ("<", Cmp::Lt),
        ]
        .into_iter()
        .find_map(|(prefix, cmp)| s.strip_prefix(prefix).map(|rest| (cmp, rest)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusMatch {
    /// `5xx`
    Class(u16),
    Compare(Cmp, u16),
}

impl StatusMatch {
    fn parse(value: &str) -> Option<Self> {
        let lower = value.to_lowercase();
        if let Some(class) = lower.strip_suffix("xx")
            && let Ok(c @ 1..=5) = class.parse::<u16>()
        {
            return Some(StatusMatch::Class(c));
        }
        let (cmp, number) = Cmp::strip(&lower).unwrap_or((Cmp::Eq, &lower));
        Some(StatusMatch::Compare(cmp, number.parse().ok()?))
    }

    fn matches(self, code: u16) -> bool {
        match self {
            StatusMatch::Class(c) => code / 100 == c,
            StatusMatch::Compare(cmp, n) => cmp.apply(code.into(), n.into()),
        }
    }
}

/// Parse `500ms`, `2s` or `1.5s` into milliseconds.
fn parse_duration_ms(value: &str) -> Option<u64> {
    let lower = value.to_lowercase();
    if let Some(ms) = lower.strip_suffix("ms") {
        return ms.parse().ok();
    }
    let secs: f64 = lower.strip_suffix('s')?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| (secs * 1000.0).round() as u64)
}

impl Term {
    fn parse(token: &str) -> Term {
        let (negate, body) = match token.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => match token.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest),
                _ => (false, token),
            },
        };
        Term {
            negate,
            kind: Self::parse_kind(body),
        }
    }

    fn parse_kind(body: &str) -> Kind {
        if let Some((cmp, rest)) = Cmp::strip(body)
            && let Some(ms) = parse_duration_ms(rest)
        {
            return Kind::Duration(cmp, ms);
        }

        if let Some((key, value)) = body.split_once(':')
            && !value.is_empty()
        {
            let lower = value.to_lowercase();
            match key.to_lowercase().as_str() {
                "src" => return Kind::Src(lower),
                "dst" => return Kind::Dst(lower),
                "method" => return Kind::Method(lower),
                "path" => return Kind::Path(lower),
                "proto" | "protocol" => {
                    let protocol = match lower.as_str() {
                        "http" => Some(Protocol::Http),
                        "postgres" | "postgresql" | "pg" | "sql" => Some(Protocol::Postgres),
                        _ => None,
                    };
                    if let Some(protocol) = protocol {
                        return Kind::Protocol(protocol);
                    }
                }
                "status" => {
                    if let Some(status) = StatusMatch::parse(value) {
                        return Kind::Status(status);
                    }
                }
                _ => {}
            }
        }

        Kind::Text(body.to_lowercase())
    }

    fn matches(&self, ex: &HttpExchange, names: &HashMap<String, String>) -> bool {
        let name_or_ip_contains = |ip: &str, needle: &str| {
            ip.to_lowercase().contains(needle)
                || names
                    .get(ip)
                    .is_some_and(|n| n.to_lowercase().contains(needle))
        };
        let hit = match &self.kind {
            Kind::Text(needle) => {
                ex.request.method.to_lowercase().contains(needle)
                    || ex.request.path.to_lowercase().contains(needle)
                    || ex
                        .response
                        .as_ref()
                        .is_some_and(|r| r.status_code.to_string().contains(needle))
                    || name_or_ip_contains(&ex.src_ip, needle)
                    || name_or_ip_contains(&ex.dst_ip, needle)
            }
            Kind::Src(needle) => name_or_ip_contains(&ex.src_ip, needle),
            Kind::Dst(needle) => name_or_ip_contains(&ex.dst_ip, needle),
            Kind::Method(needle) => ex.request.method.to_lowercase().contains(needle),
            Kind::Path(needle) => ex.request.path.to_lowercase().contains(needle),
            Kind::Status(status) => ex
                .response
                .as_ref()
                .is_some_and(|r| status.matches(r.status_code)),
            Kind::Protocol(protocol) => ex.protocol == *protocol,
            Kind::Duration(cmp, ms) => ex.duration_ms.is_some_and(|d| cmp.apply(d, *ms)),
        };
        hit != self.negate
    }
}

impl Filter {
    pub fn parse(input: &str) -> Self {
        Self {
            terms: input.split_whitespace().map(Term::parse).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn matches(&self, ex: &HttpExchange, names: &HashMap<String, String>) -> bool {
        self.terms.iter().all(|t| t.matches(ex, names))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpRequest, HttpResponse};

    fn exchange(method: &str, path: &str, status: Option<u16>, ms: Option<u64>) -> HttpExchange {
        HttpExchange {
            request: HttpRequest {
                method: method.into(),
                path: path.into(),
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
            src_ip: "10.0.0.2".into(),
            dst_ip: "10.0.0.3".into(),
            src_port: 40000,
            dst_port: 80,
            id: 0,
            protocol: Default::default(),
            timestamp_us: None,
            duration_ms: ms,
        }
    }

    fn names() -> HashMap<String, String> {
        HashMap::from([
            ("10.0.0.2".into(), "poller".into()),
            ("10.0.0.3".into(), "gateway".into()),
        ])
    }

    fn check(filter: &str, ex: &HttpExchange) -> bool {
        Filter::parse(filter).matches(ex, &names())
    }

    #[test]
    fn empty_filter_matches_everything() {
        let f = Filter::parse("   ");
        assert!(f.is_empty());
        assert!(f.matches(&exchange("GET", "/", None, None), &names()));
    }

    #[test]
    fn bare_word_searches_method_path_status_and_names() {
        let ex = exchange("POST", "/api/echo", Some(201), Some(5));
        assert!(check("post", &ex));
        assert!(check("echo", &ex));
        assert!(check("201", &ex));
        assert!(check("gateway", &ex));
        assert!(!check("delete", &ex));
    }

    #[test]
    fn terms_are_anded() {
        let ex = exchange("GET", "/health", Some(200), Some(5));
        assert!(check("get health", &ex));
        assert!(!check("get echo", &ex));
    }

    #[test]
    fn src_and_dst_match_names_and_ips() {
        let ex = exchange("GET", "/", Some(200), None);
        assert!(check("src:poll", &ex));
        assert!(check("dst:gateway", &ex));
        assert!(check("dst:10.0.0.3", &ex));
        assert!(!check("src:gateway", &ex));
    }

    #[test]
    fn status_exact_class_and_comparison() {
        let ex = exchange("GET", "/", Some(503), None);
        assert!(check("status:503", &ex));
        assert!(check("status:5xx", &ex));
        assert!(check("status:>=500", &ex));
        assert!(check("status:>400", &ex));
        assert!(!check("status:<500", &ex));
        assert!(!check("status:2xx", &ex));
    }

    #[test]
    fn status_filter_never_matches_pending() {
        let ex = exchange("GET", "/", None, None);
        assert!(!check("status:2xx", &ex));
        assert!(check("!status:2xx", &ex));
    }

    #[test]
    fn duration_comparisons() {
        let ex = exchange("GET", "/", Some(200), Some(750));
        assert!(check(">500ms", &ex));
        assert!(check("<1s", &ex));
        assert!(check("<=750ms", &ex));
        assert!(!check(">1s", &ex));
        assert!(check(">0.5s", &ex));
    }

    #[test]
    fn duration_filter_never_matches_pending() {
        let ex = exchange("GET", "/", None, None);
        assert!(!check(">1ms", &ex));
    }

    #[test]
    fn negation_with_bang_and_dash() {
        let ex = exchange("GET", "/health", Some(200), None);
        assert!(!check("!health", &ex));
        assert!(!check("-path:health", &ex));
        assert!(check("-path:echo", &ex));
    }

    #[test]
    fn lone_dash_is_literal_text() {
        let ex = exchange("GET", "/a-b", Some(200), None);
        assert!(check("-", &ex));
    }

    #[test]
    fn invalid_values_fall_back_to_text() {
        let ex = exchange("GET", "/status:abc", Some(200), None);
        assert!(check("status:abc", &ex));
        let ex = exchange("GET", "/", Some(200), None);
        assert!(!check("status:abc", &ex));
    }

    #[test]
    fn protocol_key_selects_by_protocol() {
        let http = exchange("GET", "/", Some(200), None);
        let mut sql = exchange("SELECT", "select 1", Some(200), None);
        sql.protocol = Protocol::Postgres;
        assert!(check("proto:http", &http));
        assert!(!check("proto:http", &sql));
        assert!(check("proto:pg", &sql));
        assert!(check("protocol:postgres", &sql));
        assert!(!check("proto:pg", &http));
        assert!(check(
            "proto:nonsense",
            &exchange("GET", "/proto:nonsense", None, None)
        ));
    }

    #[test]
    fn method_and_path_keys() {
        let ex = exchange("POST", "/api/echo", Some(200), None);
        assert!(check("method:post path:/api", &ex));
        assert!(!check("method:get", &ex));
    }
}
