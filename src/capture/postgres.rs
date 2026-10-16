//! PostgreSQL wire protocol (version 3) decoder.
//!
//! Each connection is recognised by its startup packet and then followed in both directions.
//! A query (simple `Q`, or extended `Execute`) becomes one [`HttpExchange`] with
//! [`Protocol::Postgres`]: the verb is the method, the SQL is the path and body, and the
//! command tag, row count and a preview of the result are the response. Connections that
//! switch to TLS cannot be decoded and are reported once.

use std::collections::{HashMap, VecDeque};
use std::fmt::Write;

use crate::capture::stream::FlowKey;
use crate::http::types::{HttpExchange, HttpRequest, HttpResponse, Protocol};

/// Capture timestamp in microseconds since the Unix epoch.
type Micros = u64;

const PROTOCOL_V3: u32 = 196_608;
const SSL_REQUEST: u32 = 80_877_103;
const GSS_ENC_REQUEST: u32 = 80_877_104;
const CANCEL_REQUEST: u32 = 80_877_102;

const MAX_STARTUP_LEN: usize = 10_000;
/// Larger messages mean the stream is not PostgreSQL or has lost sync.
const MAX_MESSAGE_LEN: usize = 256 * 1024 * 1024;
const MAX_PREVIEW_ROWS: usize = 50;
const MAX_CELL_CHARS: usize = 120;
const MAX_PATH_CHARS: usize = 300;
const MAX_BINARY_CELL_BYTES: usize = 16;
/// Requests without an answer after this long are reported as unanswered.
const PENDING_TIMEOUT_US: u64 = 30_000_000;
const VERSION: &str = "PostgreSQL";

const OID_BOOL: u32 = 16;
const OID_INT8: u32 = 20;
const OID_INT2: u32 = 21;
const OID_INT4: u32 = 23;
const OID_FLOAT4: u32 = 700;
const OID_FLOAT8: u32 = 701;
const OID_UUID: u32 = 2950;
const OID_TEXT_LIKE: [u32; 4] = [18, 19, 25, 1043];

/// True if `data` starts like a client startup packet, which identifies a PostgreSQL
/// connection.
fn looks_like_startup(data: &[u8]) -> bool {
    let Some(len) = be_u32(data, 0) else {
        return false;
    };
    let Some(code) = be_u32(data, 4) else {
        return false;
    };
    (8..=MAX_STARTUP_LEN as u32).contains(&len)
        && matches!(
            code,
            PROTOCOL_V3 | SSL_REQUEST | GSS_ENC_REQUEST | CANCEL_REQUEST
        )
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// Cursor over a message payload; every read is bounds checked.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let slice = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }

    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.bytes(2)?.try_into().ok()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.bytes(4)?.try_into().ok()?))
    }

    fn cstring(&mut self) -> Option<String> {
        let rest = self.data.get(self.pos..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        self.pos += end + 1;
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Connect,
    Simple,
    Extended,
}

#[derive(Debug, Clone)]
struct Column {
    name: String,
    type_oid: u32,
}

#[derive(Debug)]
struct Statement {
    query: String,
    param_oids: Vec<u32>,
}

#[derive(Debug)]
struct Portal {
    statement: String,
    params: Vec<String>,
    result_formats: Vec<i16>,
}

/// Fields of an `ErrorResponse`, keyed by their one-letter code.
#[derive(Debug, Default)]
struct ServerError {
    fields: Vec<(char, String)>,
}

impl ServerError {
    fn parse(payload: &[u8]) -> Self {
        let mut reader = Reader::new(payload);
        let mut fields = Vec::new();
        while let Some(code) = reader.u8() {
            if code == 0 {
                break;
            }
            let Some(value) = reader.cstring() else { break };
            fields.push((code as char, value));
        }
        Self { fields }
    }

    fn get(&self, code: char) -> Option<&str> {
        self.fields
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_str())
    }
}

/// A request waiting for the server to answer.
#[derive(Debug)]
struct Pending {
    kind: Kind,
    sql: String,
    params: Vec<String>,
    statement: String,
    result_formats: Vec<i16>,
    start: Micros,
    columns: Vec<Column>,
    preview: Vec<Vec<String>>,
    row_count: usize,
    tags: Vec<String>,
    error: Option<ServerError>,
}

impl Pending {
    fn new(kind: Kind, sql: String, start: Micros) -> Self {
        Self {
            kind,
            sql,
            params: Vec::new(),
            statement: String::new(),
            result_formats: Vec::new(),
            start,
            columns: Vec::new(),
            preview: Vec::new(),
            row_count: 0,
            tags: Vec::new(),
            error: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Phase {
    /// Before the startup message: untyped packets.
    Startup,
    /// Typed messages in both directions.
    Active,
}

/// One followed connection. `client` is the client-to-server flow.
#[derive(Debug)]
struct Conn {
    client: FlowKey,
    phase: Phase,
    started: Micros,
    user: Option<String>,
    database: Option<String>,
    application: Option<String>,
    /// An SSL/GSS request was sent and the server's one-byte answer is awaited.
    awaiting_encryption_answer: bool,
    /// The server accepted TLS: nothing more can be decoded.
    encrypted: bool,
    /// Not PostgreSQL, or sync was lost; ignore the rest of the stream.
    dead: bool,
    statements: HashMap<String, Statement>,
    portals: HashMap<String, Portal>,
    pending: VecDeque<Pending>,
    handshake: Option<Pending>,
    columns: Vec<Column>,
}

impl Conn {
    fn new(client: FlowKey, now: Micros) -> Self {
        Self {
            client,
            phase: Phase::Startup,
            started: now,
            user: None,
            database: None,
            application: None,
            awaiting_encryption_answer: false,
            encrypted: false,
            dead: false,
            statements: HashMap::new(),
            portals: HashMap::new(),
            pending: VecDeque::new(),
            handshake: None,
            columns: Vec::new(),
        }
    }

    /// Process client-to-server bytes; returns how many were consumed.
    fn client_data(&mut self, data: &[u8], now: Micros) -> usize {
        let mut offset = 0;
        loop {
            if self.dead || self.encrypted {
                return data.len();
            }
            let rest = &data[offset..];
            match self.phase {
                Phase::Startup => {
                    let Some(len) = be_u32(rest, 0).map(|l| l as usize) else {
                        break;
                    };
                    if !(8..=MAX_STARTUP_LEN).contains(&len) {
                        self.dead = true;
                        continue;
                    }
                    if rest.len() < len {
                        break;
                    }
                    self.startup_packet(&rest[4..len], now);
                    offset += len;
                }
                Phase::Active => {
                    let Some((kind, payload, total)) = next_message(rest, &mut self.dead) else {
                        if self.dead {
                            return data.len();
                        }
                        break;
                    };
                    self.client_message(kind, payload, now);
                    offset += total;
                }
            }
        }
        offset
    }

    fn startup_packet(&mut self, body: &[u8], now: Micros) {
        let mut reader = Reader::new(body);
        let Some(code) = reader.i32().map(|c| c as u32) else {
            self.dead = true;
            return;
        };
        match code {
            SSL_REQUEST | GSS_ENC_REQUEST => self.awaiting_encryption_answer = true,
            PROTOCOL_V3 => {
                while let Some(key) = reader.cstring() {
                    if key.is_empty() {
                        break;
                    }
                    let Some(value) = reader.cstring() else { break };
                    match key.as_str() {
                        "user" => self.user = Some(value),
                        "database" => self.database = Some(value),
                        "application_name" => self.application = Some(value),
                        _ => {}
                    }
                }
                self.phase = Phase::Active;
                self.handshake = Some(Pending::new(Kind::Connect, String::new(), now));
            }
            // Cancel requests and unknown packets carry nothing to show.
            _ => self.dead = true,
        }
    }

    fn client_message(&mut self, kind: u8, payload: &[u8], now: Micros) {
        let mut reader = Reader::new(payload);
        match kind {
            b'Q' => {
                if let Some(sql) = reader.cstring() {
                    self.pending.push_back(Pending::new(Kind::Simple, sql, now));
                }
            }
            b'P' => {
                let (Some(name), Some(query), Some(count)) =
                    (reader.cstring(), reader.cstring(), reader.i16())
                else {
                    return;
                };
                let param_oids = (0..count.max(0))
                    .map_while(|_| reader.i32().map(|o| o as u32))
                    .collect();
                self.statements
                    .insert(name, Statement { query, param_oids });
            }
            b'B' => self.bind(&mut reader),
            b'E' => {
                let Some(portal_name) = reader.cstring() else {
                    return;
                };
                let (sql, statement, params, result_formats) = match self.portals.get(&portal_name)
                {
                    Some(portal) => (
                        self.statements
                            .get(&portal.statement)
                            .map_or_else(|| "<unknown statement>".to_string(), |s| s.query.clone()),
                        portal.statement.clone(),
                        portal.params.clone(),
                        portal.result_formats.clone(),
                    ),
                    None => (
                        "<unknown portal>".to_string(),
                        String::new(),
                        vec![],
                        vec![],
                    ),
                };
                let mut pending = Pending::new(Kind::Extended, sql, now);
                pending.statement = statement;
                pending.params = params;
                pending.result_formats = result_formats;
                self.pending.push_back(pending);
            }
            b'C' => {
                let (Some(what), Some(name)) = (reader.u8(), reader.cstring()) else {
                    return;
                };
                if what == b'S' {
                    self.statements.remove(&name);
                } else {
                    self.portals.remove(&name);
                }
            }
            // Terminate, Sync, Describe, Flush, password messages and COPY data carry no
            // query; passwords in particular are never read.
            _ => {}
        }
    }

    fn bind(&mut self, reader: &mut Reader) {
        let (Some(portal), Some(statement), Some(format_count)) =
            (reader.cstring(), reader.cstring(), reader.i16())
        else {
            return;
        };
        let formats: Vec<i16> = (0..format_count.max(0))
            .map_while(|_| reader.i16())
            .collect();
        let Some(param_count) = reader.i16() else {
            return;
        };
        let oids = self
            .statements
            .get(&statement)
            .map(|s| s.param_oids.clone())
            .unwrap_or_default();

        let mut params = Vec::new();
        for index in 0..param_count.max(0) as usize {
            let Some(len) = reader.i32() else { return };
            let value = if len < 0 {
                "NULL".to_string()
            } else {
                let Some(bytes) = reader.bytes(len as usize) else {
                    return;
                };
                let binary = format_for(&formats, index) == 1;
                let oid = oids.get(index).copied().unwrap_or(0);
                quote_if_text(&format_value(bytes, binary, oid), binary, oid)
            };
            params.push(value);
        }
        let result_count = reader.i16().unwrap_or(0);
        let result_formats = (0..result_count.max(0))
            .map_while(|_| reader.i16())
            .collect();
        self.portals.insert(
            portal,
            Portal {
                statement,
                params,
                result_formats,
            },
        );
    }

    /// Process server-to-client bytes; returns how many were consumed and the finished
    /// exchanges.
    fn server_data(&mut self, data: &[u8], now: Micros, out: &mut Vec<HttpExchange>) -> usize {
        let mut offset = 0;
        if self.awaiting_encryption_answer {
            let Some(&answer) = data.first() else {
                return 0;
            };
            self.awaiting_encryption_answer = false;
            match answer {
                b'S' | b'G' => {
                    self.encrypted = true;
                    out.push(self.encrypted_exchange(now));
                    return data.len();
                }
                // The server declined; the client continues in plain text.
                b'N' => offset = 1,
                _ => {
                    self.dead = true;
                    return data.len();
                }
            }
        }

        loop {
            if self.dead || self.encrypted {
                return data.len();
            }
            let Some((kind, payload, total)) = next_message(&data[offset..], &mut self.dead) else {
                if self.dead {
                    return data.len();
                }
                break;
            };
            self.server_message(kind, payload, now, out);
            offset += total;
        }
        offset
    }

    fn server_message(
        &mut self,
        kind: u8,
        payload: &[u8],
        now: Micros,
        out: &mut Vec<HttpExchange>,
    ) {
        match kind {
            b'T' => self.columns = parse_row_description(payload),
            b'n' => self.columns.clear(),
            b'D' => self.data_row(payload),
            b'C' => {
                let tag = Reader::new(payload).cstring().unwrap_or_default();
                self.command_complete(tag, now, out);
            }
            b'I' => self.command_complete("EMPTY QUERY".to_string(), now, out),
            b's' => self.command_complete("SUSPENDED".to_string(), now, out),
            b'E' => self.error(ServerError::parse(payload), now, out),
            b'Z' => self.ready_for_query(now, out),
            // Authentication, parameter status, notices, parse/bind completion and COPY
            // responses do not change what is shown.
            _ => {}
        }
    }

    fn data_row(&mut self, payload: &[u8]) {
        let Some(head) = self.pending.front_mut() else {
            return;
        };
        if head.columns.is_empty() {
            head.columns = self.columns.clone();
        }
        head.row_count += 1;
        if head.preview.len() >= MAX_PREVIEW_ROWS {
            return;
        }

        let mut reader = Reader::new(payload);
        let Some(count) = reader.i16() else { return };
        let mut row = Vec::new();
        for index in 0..count.max(0) as usize {
            let Some(len) = reader.i32() else { break };
            if len < 0 {
                row.push("NULL".to_string());
                continue;
            }
            let Some(bytes) = reader.bytes(len as usize) else {
                break;
            };
            let binary = format_for(&head.result_formats, index) == 1;
            let oid = head.columns.get(index).map_or(0, |c| c.type_oid);
            row.push(truncate(&format_value(bytes, binary, oid), MAX_CELL_CHARS));
        }
        head.preview.push(row);
    }

    fn command_complete(&mut self, tag: String, now: Micros, out: &mut Vec<HttpExchange>) {
        let Some(head) = self.pending.front_mut() else {
            return;
        };
        head.tags.push(tag);
        // A simple query may run several statements; it ends at ReadyForQuery.
        if head.kind == Kind::Extended
            && let Some(done) = self.pending.pop_front()
        {
            out.push(self.exchange(done, Some(now)));
        }
    }

    fn error(&mut self, error: ServerError, now: Micros, out: &mut Vec<HttpExchange>) {
        if let Some(handshake) = self.handshake.take() {
            let mut failed = handshake;
            failed.error = Some(error);
            out.push(self.exchange(failed, Some(now)));
            return;
        }
        let Some(head) = self.pending.front_mut() else {
            return;
        };
        head.error = Some(error);
        if head.kind == Kind::Extended
            && let Some(done) = self.pending.pop_front()
        {
            out.push(self.exchange(done, Some(now)));
        }
    }

    fn ready_for_query(&mut self, now: Micros, out: &mut Vec<HttpExchange>) {
        if let Some(handshake) = self.handshake.take() {
            out.push(self.exchange(handshake, Some(now)));
        }
        // Whatever is still queued is either a finished simple query or a pipelined
        // statement the server skipped after an error.
        while let Some(item) = self.pending.pop_front() {
            if item.kind == Kind::Simple || item.error.is_some() {
                out.push(self.exchange(item, Some(now)));
            }
        }
        self.columns.clear();
    }

    fn encrypted_exchange(&self, now: Micros) -> HttpExchange {
        let mut pending = Pending::new(Kind::Connect, String::new(), self.started);
        pending.tags.push("TLS".to_string());
        let mut exchange = self.exchange(pending, Some(now));
        exchange.request.path = "(TLS encrypted, content not decoded)".to_string();
        exchange
    }

    /// Build the exchange for a request; `end` is `None` when the server never answered.
    fn exchange(&self, pending: Pending, end: Option<Micros>) -> HttpExchange {
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut push = |name: &str, value: Option<&str>| {
            if let Some(value) = value.filter(|v| !v.is_empty()) {
                headers.push((name.to_string(), value.to_string()));
            }
        };
        let (method, path, body) = match pending.kind {
            Kind::Connect => {
                push("User", self.user.as_deref());
                push("Database", self.database.as_deref());
                push("Application", self.application.as_deref());
                (
                    "CONNECT".to_string(),
                    format!(
                        "{}@{}",
                        self.user.as_deref().unwrap_or("?"),
                        self.database.as_deref().unwrap_or("?")
                    ),
                    None,
                )
            }
            Kind::Simple | Kind::Extended => {
                let style = if pending.kind == Kind::Simple {
                    "simple"
                } else {
                    "extended"
                };
                push("Protocol", Some(style));
                push("User", self.user.as_deref());
                push("Database", self.database.as_deref());
                push("Application", self.application.as_deref());
                push("Statement", Some(&pending.statement));
                let mut body = pending.sql.clone();
                if !pending.params.is_empty() {
                    body.push_str("\n\n-- parameters\n");
                    for (index, value) in pending.params.iter().enumerate() {
                        let _ = writeln!(body, "-- ${} = {value}", index + 1);
                    }
                }
                (
                    sql_verb(&pending.sql),
                    one_line(&pending.sql, MAX_PATH_CHARS),
                    Some(body.into_bytes()),
                )
            }
        };

        let response = end.map(|_| self.response(&pending));
        let duration_ms = end.map(|e| e.saturating_sub(pending.start) / 1000);

        HttpExchange {
            id: 0,
            protocol: Protocol::Postgres,
            request: HttpRequest {
                method,
                path,
                version: VERSION.to_string(),
                headers,
                body,
            },
            response,
            src_ip: self.client.src_ip.to_string(),
            dst_ip: self.client.dst_ip.to_string(),
            src_port: self.client.src_port,
            dst_port: self.client.dst_port,
            timestamp_us: Some(pending.start),
            duration_ms,
        }
    }

    fn response(&self, pending: &Pending) -> HttpResponse {
        let mut headers = Vec::new();
        if let Some(error) = &pending.error {
            for (name, code) in [
                ("Severity", 'S'),
                ("SQLSTATE", 'C'),
                ("Message", 'M'),
                ("Detail", 'D'),
                ("Hint", 'H'),
                ("Position", 'P'),
            ] {
                if let Some(value) = error.get(code) {
                    headers.push((name.to_string(), value.to_string()));
                }
            }
            let sqlstate = error.get('C').unwrap_or("?");
            let message = error.get('M').unwrap_or("error");
            return HttpResponse {
                status_code: 500,
                reason: format!("ERROR {sqlstate}"),
                version: VERSION.to_string(),
                headers,
                body: Some(
                    format!("{}: {message}", error.get('S').unwrap_or("ERROR")).into_bytes(),
                ),
            };
        }

        let tag = match pending.kind {
            Kind::Connect if pending.tags.is_empty() => "authenticated".to_string(),
            _ => pending.tags.join("; "),
        };
        if !tag.is_empty() {
            headers.push(("Command-Tag".to_string(), tag.clone()));
        }
        if pending.kind != Kind::Connect {
            headers.push(("Rows".to_string(), pending.row_count.to_string()));
        }
        HttpResponse {
            status_code: 200,
            reason: tag,
            version: VERSION.to_string(),
            headers,
            body: result_table(pending).map(String::into_bytes),
        }
    }

    fn sweep(&mut self, now: Micros) -> Vec<HttpExchange> {
        let mut stale = Vec::new();
        if self
            .handshake
            .as_ref()
            .is_some_and(|h| now.saturating_sub(h.start) > PENDING_TIMEOUT_US)
        {
            stale.extend(self.handshake.take());
        }
        let mut kept = VecDeque::new();
        for item in std::mem::take(&mut self.pending) {
            if now.saturating_sub(item.start) > PENDING_TIMEOUT_US {
                stale.push(item);
            } else {
                kept.push_back(item);
            }
        }
        self.pending = kept;
        stale.into_iter().map(|p| self.exchange(p, None)).collect()
    }
}

/// Split the next typed message off `data`: `(type, payload, total bytes)`. Marks the
/// connection dead on an impossible length.
fn next_message<'a>(data: &'a [u8], dead: &mut bool) -> Option<(u8, &'a [u8], usize)> {
    let kind = *data.first()?;
    let len = be_u32(data, 1)? as usize;
    if !(4..=MAX_MESSAGE_LEN).contains(&len) {
        *dead = true;
        return None;
    }
    let payload = data.get(5..1 + len)?;
    Some((kind, payload, 1 + len))
}

fn parse_row_description(payload: &[u8]) -> Vec<Column> {
    let mut reader = Reader::new(payload);
    let Some(count) = reader.i16() else {
        return Vec::new();
    };
    let mut columns = Vec::new();
    for _ in 0..count.max(0) {
        let Some(name) = reader.cstring() else { break };
        // table oid, attribute number, type oid, type size, type modifier, format code
        let (Some(_), Some(_), Some(type_oid), Some(_), Some(_), Some(_)) = (
            reader.i32(),
            reader.i16(),
            reader.i32(),
            reader.i16(),
            reader.i32(),
            reader.i16(),
        ) else {
            break;
        };
        columns.push(Column {
            name,
            type_oid: type_oid as u32,
        });
    }
    columns
}

/// Format code for column or parameter `index`: one code applies to all, none means text.
fn format_for(formats: &[i16], index: usize) -> i16 {
    match formats {
        [] => 0,
        [one] => *one,
        many => many.get(index).copied().unwrap_or(0),
    }
}

fn format_value(bytes: &[u8], binary: bool, oid: u32) -> String {
    if !binary {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    match (oid, bytes.len()) {
        (OID_BOOL, 1) => (bytes[0] != 0).to_string(),
        (OID_INT2, 2) => i16::from_be_bytes([bytes[0], bytes[1]]).to_string(),
        (OID_INT4, 4) => i32::from_be_bytes(bytes.try_into().unwrap_or_default()).to_string(),
        (OID_INT8, 8) => i64::from_be_bytes(bytes.try_into().unwrap_or_default()).to_string(),
        (OID_FLOAT4, 4) => f32::from_be_bytes(bytes.try_into().unwrap_or_default()).to_string(),
        (OID_FLOAT8, 8) => f64::from_be_bytes(bytes.try_into().unwrap_or_default()).to_string(),
        (OID_UUID, 16) => {
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!(
                "{}-{}-{}-{}-{}",
                &hex[..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..]
            )
        }
        (oid, _) if OID_TEXT_LIKE.contains(&oid) => String::from_utf8_lossy(bytes).into_owned(),
        _ => {
            let shown: String = bytes
                .iter()
                .take(MAX_BINARY_CELL_BYTES)
                .map(|b| format!("{b:02x}"))
                .collect();
            let more = if bytes.len() > MAX_BINARY_CELL_BYTES {
                "…"
            } else {
                ""
            };
            format!("\\x{shown}{more}")
        }
    }
}

/// Parameters in text format are shown quoted so they read like SQL literals.
fn quote_if_text(value: &str, binary: bool, oid: u32) -> String {
    let is_number = value.parse::<f64>().is_ok();
    if binary && oid != 0 && !OID_TEXT_LIKE.contains(&oid) || is_number {
        return truncate(value, MAX_CELL_CHARS);
    }
    format!("'{}'", truncate(&value.replace('\'', "''"), MAX_CELL_CHARS))
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// The statement's first keyword in upper case, skipping comments and parentheses.
fn sql_verb(sql: &str) -> String {
    let mut rest = sql.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("--") {
            rest = after.split_once('\n').map_or("", |(_, r)| r).trim_start();
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.split_once("*/").map_or("", |(_, r)| r).trim_start();
        } else if let Some(after) = rest.strip_prefix('(') {
            rest = after.trim_start();
        } else {
            break;
        }
    }
    let verb: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_uppercase();
    if verb.is_empty() {
        "SQL".to_string()
    } else {
        verb
    }
}

/// Collapse whitespace so a query fits on one list row.
fn one_line(sql: &str, max_chars: usize) -> String {
    let collapsed = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, max_chars)
}

/// Plain-text table of the first rows of the result, if the query returned columns.
fn result_table(pending: &Pending) -> Option<String> {
    if pending.columns.is_empty() && pending.preview.is_empty() {
        return None;
    }
    let mut out = String::new();
    let header: Vec<&str> = pending.columns.iter().map(|c| c.name.as_str()).collect();
    out.push_str(&header.join(" | "));
    out.push('\n');
    out.push_str(&"-".repeat(out.trim_end().chars().count().max(3)));
    out.push('\n');
    for row in &pending.preview {
        out.push_str(&row.join(" | "));
        out.push('\n');
    }
    if pending.row_count > pending.preview.len() {
        let _ = writeln!(
            out,
            "… {} more rows",
            pending.row_count - pending.preview.len()
        );
    }
    let _ = write!(
        out,
        "({} row{})",
        pending.row_count,
        if pending.row_count == 1 { "" } else { "s" }
    );
    Some(out)
}

/// Follows every PostgreSQL connection seen on the wire.
#[derive(Default)]
pub struct PgTracker {
    conns: HashMap<FlowKey, Conn>,
}

impl PgTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle accumulated stream bytes for `flow`. Returns `None` if the flow is not
    /// PostgreSQL, otherwise the number of bytes consumed and any finished exchanges.
    pub fn process(
        &mut self,
        flow: &FlowKey,
        data: &[u8],
        now: Micros,
    ) -> Option<(usize, Vec<HttpExchange>)> {
        let mut out = Vec::new();

        if let Some(conn) = self.conns.get_mut(flow) {
            let consumed = conn.client_data(data, now);
            return Some((consumed, out));
        }
        let reverse = flow.reverse();
        if let Some(conn) = self.conns.get_mut(&reverse) {
            let consumed = conn.server_data(data, now, &mut out);
            if conn.dead {
                self.conns.remove(&reverse);
            }
            return Some((consumed, out));
        }
        if looks_like_startup(data) {
            let mut conn = Conn::new(flow.clone(), now);
            let consumed = conn.client_data(data, now);
            self.conns.insert(flow.clone(), conn);
            return Some((consumed, out));
        }
        None
    }

    /// Flush requests that have waited longer than the timeout as unanswered exchanges.
    pub fn sweep(&mut self, now: Micros) -> Vec<HttpExchange> {
        self.conns.values_mut().flat_map(|c| c.sweep(now)).collect()
    }

    /// Drop the oldest connections beyond `max`.
    pub fn cleanup(&mut self, max: usize) {
        while self.conns.len() > max {
            let Some(oldest) = self
                .conns
                .values()
                .min_by_key(|c| c.started)
                .map(|c| c.client.clone())
            else {
                break;
            };
            self.conns.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> FlowKey {
        FlowKey {
            src_ip: "10.0.0.2".parse().expect("ip"),
            src_port: 40000,
            dst_ip: "10.0.0.3".parse().expect("ip"),
            dst_port: 5432,
        }
    }

    fn cstr(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    }

    fn msg(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![kind];
        v.extend((payload.len() as u32 + 4).to_be_bytes());
        v.extend(payload);
        v
    }

    fn startup(params: &[(&str, &str)]) -> Vec<u8> {
        let mut body = PROTOCOL_V3.to_be_bytes().to_vec();
        for (k, v) in params {
            body.extend(cstr(k));
            body.extend(cstr(v));
        }
        body.push(0);
        let mut v = (body.len() as u32 + 4).to_be_bytes().to_vec();
        v.extend(body);
        v
    }

    fn ssl_request() -> Vec<u8> {
        let mut v = 8u32.to_be_bytes().to_vec();
        v.extend(SSL_REQUEST.to_be_bytes());
        v
    }

    fn row_description(columns: &[&str]) -> Vec<u8> {
        let mut p = (columns.len() as i16).to_be_bytes().to_vec();
        for name in columns {
            p.extend(cstr(name));
            p.extend(0i32.to_be_bytes());
            p.extend(0i16.to_be_bytes());
            p.extend(25i32.to_be_bytes());
            p.extend((-1i16).to_be_bytes());
            p.extend((-1i32).to_be_bytes());
            p.extend(0i16.to_be_bytes());
        }
        msg(b'T', &p)
    }

    fn data_row(values: &[Option<&str>]) -> Vec<u8> {
        let mut p = (values.len() as i16).to_be_bytes().to_vec();
        for value in values {
            match value {
                Some(v) => {
                    p.extend((v.len() as i32).to_be_bytes());
                    p.extend(v.as_bytes());
                }
                None => p.extend((-1i32).to_be_bytes()),
            }
        }
        msg(b'D', &p)
    }

    fn error_response(code: &str, message: &str) -> Vec<u8> {
        let mut p = Vec::new();
        for (field, value) in [(b'S', "ERROR"), (b'C', code), (b'M', message)] {
            p.push(field);
            p.extend(cstr(value));
        }
        p.push(0);
        msg(b'E', &p)
    }

    fn ready() -> Vec<u8> {
        msg(b'Z', b"I")
    }

    /// Feed one direction and return the finished exchanges.
    fn client(tracker: &mut PgTracker, data: &[u8], at: Micros) -> usize {
        tracker.process(&flow(), data, at).expect("pg").0
    }

    fn server(tracker: &mut PgTracker, data: &[u8], at: Micros) -> Vec<HttpExchange> {
        tracker.process(&flow().reverse(), data, at).expect("pg").1
    }

    fn connected() -> PgTracker {
        let mut t = PgTracker::new();
        client(
            &mut t,
            &startup(&[
                ("user", "app"),
                ("database", "shop"),
                ("application_name", "api"),
            ]),
            1_000_000,
        );
        let done = server(
            &mut t,
            &[msg(b'R', &0i32.to_be_bytes()), ready()].concat(),
            1_040_000,
        );
        assert_eq!(done.len(), 1);
        t
    }

    #[test]
    fn startup_is_recognised_and_other_traffic_is_not() {
        assert!(looks_like_startup(&startup(&[("user", "a")])));
        assert!(looks_like_startup(&ssl_request()));
        assert!(!looks_like_startup(b"GET / HTTP/1.1\r\n\r\n"));
        assert!(!looks_like_startup(&[0, 0]));
        let mut t = PgTracker::new();
        assert!(t.process(&flow(), b"GET / HTTP/1.1\r\n\r\n", 0).is_none());
    }

    #[test]
    fn handshake_becomes_a_connect_exchange() {
        let mut t = PgTracker::new();
        let startup = startup(&[
            ("user", "app"),
            ("database", "shop"),
            ("application_name", "api"),
        ]);
        assert_eq!(client(&mut t, &startup, 1_000_000), startup.len());
        let done = server(
            &mut t,
            &[msg(b'R', &0i32.to_be_bytes()), ready()].concat(),
            1_040_000,
        );

        let ex = &done[0];
        assert_eq!(ex.protocol, Protocol::Postgres);
        assert_eq!(ex.request.method, "CONNECT");
        assert_eq!(ex.request.path, "app@shop");
        assert_eq!(ex.dst_port, 5432);
        assert_eq!(ex.duration_ms, Some(40));
        let response = ex.response.as_ref().expect("response");
        assert_eq!(
            (response.status_code, response.reason.as_str()),
            (200, "authenticated")
        );
        assert!(
            ex.request
                .headers
                .contains(&("Application".into(), "api".into()))
        );
    }

    #[test]
    fn failed_login_is_an_error_exchange() {
        let mut t = PgTracker::new();
        client(&mut t, &startup(&[("user", "app")]), 0);
        let done = server(
            &mut t,
            &error_response("28P01", "password authentication failed"),
            5_000,
        );
        let response = done[0].response.as_ref().expect("response");
        assert_eq!(response.status_code, 500);
        assert_eq!(response.reason, "ERROR 28P01");
    }

    #[test]
    fn simple_query_with_rows() {
        let mut t = connected();
        client(
            &mut t,
            &msg(b'Q', &cstr("SELECT id, name FROM users")),
            2_000_000,
        );
        let reply = [
            row_description(&["id", "name"]),
            data_row(&[Some("1"), Some("ana")]),
            data_row(&[Some("2"), None]),
            msg(b'C', &cstr("SELECT 2")),
            ready(),
        ]
        .concat();
        let done = server(&mut t, &reply, 2_012_000);

        assert_eq!(done.len(), 1);
        let ex = &done[0];
        assert_eq!(ex.request.method, "SELECT");
        assert_eq!(ex.request.path, "SELECT id, name FROM users");
        assert_eq!(ex.duration_ms, Some(12));
        let response = ex.response.as_ref().expect("response");
        assert_eq!(
            (response.status_code, response.reason.as_str()),
            (200, "SELECT 2")
        );
        assert!(response.headers.contains(&("Rows".into(), "2".into())));
        let table = String::from_utf8(response.body.clone().expect("body")).expect("utf8");
        assert!(table.starts_with("id | name\n"));
        assert!(table.contains("1 | ana"));
        assert!(table.contains("2 | NULL"));
        assert!(table.ends_with("(2 rows)"));
    }

    #[test]
    fn statement_without_result_set_has_tag_only() {
        let mut t = connected();
        client(&mut t, &msg(b'Q', &cstr("INSERT INTO t VALUES (1)")), 0);
        let done = server(
            &mut t,
            &[msg(b'C', &cstr("INSERT 0 1")), ready()].concat(),
            3_000,
        );
        let response = done[0].response.as_ref().expect("response");
        assert_eq!(response.reason, "INSERT 0 1");
        assert!(response.body.is_none());
    }

    #[test]
    fn server_error_is_a_500_with_details() {
        let mut t = connected();
        client(&mut t, &msg(b'Q', &cstr("INSERT INTO t VALUES (1)")), 0);
        let done = server(
            &mut t,
            &[error_response("23505", "duplicate key value"), ready()].concat(),
            9_000,
        );
        let response = done[0].response.as_ref().expect("response");
        assert_eq!(response.status_code, 500);
        assert_eq!(response.reason, "ERROR 23505");
        assert!(
            response
                .headers
                .contains(&("SQLSTATE".into(), "23505".into()))
        );
        assert!(
            response
                .headers
                .contains(&("Message".into(), "duplicate key value".into()))
        );
    }

    #[test]
    fn extended_query_shows_parameters() {
        let mut t = connected();
        let parse = {
            let mut p = cstr("s1");
            p.extend(cstr("SELECT * FROM users WHERE id = $1 AND name = $2"));
            p.extend(2i16.to_be_bytes());
            p.extend(23i32.to_be_bytes());
            p.extend(25i32.to_be_bytes());
            msg(b'P', &p)
        };
        let bind = {
            let mut p = cstr("");
            p.extend(cstr("s1"));
            p.extend(0i16.to_be_bytes());
            p.extend(2i16.to_be_bytes());
            for v in ["42", "o'neil"] {
                p.extend((v.len() as i32).to_be_bytes());
                p.extend(v.as_bytes());
            }
            p.extend(0i16.to_be_bytes());
            msg(b'B', &p)
        };
        let execute = {
            let mut p = cstr("");
            p.extend(0i32.to_be_bytes());
            msg(b'E', &p)
        };
        let sync = msg(b'S', &[]);
        client(&mut t, &[parse, bind, execute, sync].concat(), 4_000_000);

        let reply = [
            msg(b'1', &[]),
            msg(b'2', &[]),
            row_description(&["id"]),
            data_row(&[Some("42")]),
            msg(b'C', &cstr("SELECT 1")),
            ready(),
        ]
        .concat();
        let done = server(&mut t, &reply, 4_002_000);

        assert_eq!(done.len(), 1);
        let ex = &done[0];
        assert_eq!(ex.request.method, "SELECT");
        assert!(
            ex.request
                .headers
                .contains(&("Protocol".into(), "extended".into()))
        );
        assert!(
            ex.request
                .headers
                .contains(&("Statement".into(), "s1".into()))
        );
        let body = String::from_utf8(ex.request.body.clone().expect("body")).expect("utf8");
        assert!(body.starts_with("SELECT * FROM users WHERE id = $1 AND name = $2"));
        assert!(body.contains("-- $1 = 42"));
        assert!(body.contains("-- $2 = 'o''neil'"));
        assert_eq!(ex.duration_ms, Some(2));
    }

    #[test]
    fn failed_statement_drops_the_pipelined_ones_after_it() {
        let mut t = connected();
        let parse = |name: &str, sql: &str| {
            let mut p = cstr(name);
            p.extend(cstr(sql));
            p.extend(0i16.to_be_bytes());
            msg(b'P', &p)
        };
        let bind = |name: &str| {
            let mut p = cstr(name);
            p.extend(cstr(name));
            p.extend(0i16.to_be_bytes());
            p.extend(0i16.to_be_bytes());
            p.extend(0i16.to_be_bytes());
            msg(b'B', &p)
        };
        let execute = |portal: &str| {
            let mut p = cstr(portal);
            p.extend(0i32.to_be_bytes());
            msg(b'E', &p)
        };
        let batch = [
            parse("a", "INSERT INTO t VALUES (1)"),
            bind("a"),
            execute("a"),
            parse("b", "INSERT INTO t VALUES (2)"),
            bind("b"),
            execute("b"),
            msg(b'S', &[]),
        ]
        .concat();
        client(&mut t, &batch, 0);

        let reply = [error_response("23505", "duplicate"), ready()].concat();
        let done = server(&mut t, &reply, 1_000);

        assert_eq!(
            done.len(),
            1,
            "the skipped second statement is not reported"
        );
        assert_eq!(
            done[0].response.as_ref().expect("response").status_code,
            500
        );
    }

    #[test]
    fn messages_split_across_packets_wait_for_the_rest() {
        let mut t = connected();
        let query = msg(b'Q', &cstr("SELECT 1"));
        let (first, second) = query.split_at(6);
        assert_eq!(
            client(&mut t, first, 0),
            0,
            "incomplete message is not consumed"
        );
        assert_eq!(client(&mut t, &query, 0), query.len());
        let _ = second;
        let done = server(
            &mut t,
            &[msg(b'C', &cstr("SELECT 0")), ready()].concat(),
            1_000,
        );
        assert_eq!(done.len(), 1);
    }

    #[test]
    fn tls_connection_is_reported_once_and_not_decoded() {
        let mut t = PgTracker::new();
        client(&mut t, &ssl_request(), 1_000_000);
        let done = server(&mut t, b"S", 1_003_000);

        assert_eq!(done.len(), 1);
        assert_eq!(done[0].request.method, "CONNECT");
        assert!(done[0].request.path.contains("TLS"));
        assert_eq!(done[0].response.as_ref().expect("response").reason, "TLS");

        // Whatever follows is encrypted and ignored.
        assert_eq!(
            client(&mut t, &[0x16, 0x03, 0x01, 0x00, 0x10], 1_004_000),
            5
        );
        assert!(server(&mut t, &[0x16, 0x03, 0x03], 1_005_000).is_empty());
    }

    #[test]
    fn declined_ssl_request_continues_in_plain_text() {
        let mut t = PgTracker::new();
        client(&mut t, &ssl_request(), 0);
        assert!(server(&mut t, b"N", 1_000).is_empty());
        client(
            &mut t,
            &startup(&[("user", "app"), ("database", "db")]),
            2_000,
        );
        let done = server(&mut t, &ready(), 3_000);
        assert_eq!(done[0].request.path, "app@db");
    }

    #[test]
    fn unanswered_queries_are_flushed_by_sweep() {
        let mut t = connected();
        client(&mut t, &msg(b'Q', &cstr("SELECT pg_sleep(100)")), 5_000_000);
        assert!(t.sweep(10_000_000).is_empty());
        let stale = t.sweep(40_000_000);
        assert_eq!(stale.len(), 1);
        assert!(stale[0].response.is_none());
        assert_eq!(stale[0].duration_ms, None);
        assert!(t.sweep(100_000_000).is_empty(), "flushed once");
    }

    #[test]
    fn garbage_after_startup_stops_decoding_without_panicking() {
        let mut t = connected();
        let consumed = client(&mut t, &[b'Q', 0xff, 0xff, 0xff, 0xff, 1, 2, 3], 0);
        assert_eq!(consumed, 8);
    }

    #[test]
    fn verbs_skip_comments_and_parentheses() {
        assert_eq!(sql_verb("  select 1"), "SELECT");
        assert_eq!(sql_verb("-- note\nUPDATE t SET a=1"), "UPDATE");
        assert_eq!(sql_verb("/* hint */ delete from t"), "DELETE");
        assert_eq!(sql_verb("(SELECT 1) UNION (SELECT 2)"), "SELECT");
        assert_eq!(sql_verb("42"), "SQL");
    }

    #[test]
    fn one_line_collapses_whitespace_and_truncates() {
        assert_eq!(
            one_line("SELECT  a,\n   b\tFROM t", 100),
            "SELECT a, b FROM t"
        );
        let long = "x".repeat(400);
        let short = one_line(&long, 10);
        assert_eq!(short.chars().count(), 10);
        assert!(short.ends_with('…'));
    }

    #[test]
    fn binary_values_are_decoded_by_type() {
        assert_eq!(format_value(&7i32.to_be_bytes(), true, OID_INT4), "7");
        assert_eq!(format_value(&(-3i64).to_be_bytes(), true, OID_INT8), "-3");
        assert_eq!(format_value(&[1], true, OID_BOOL), "true");
        assert_eq!(format_value(b"abc", true, 25), "abc");
        assert_eq!(format_value(&[0xde, 0xad], true, 17), "\\xdead");
        assert_eq!(format_value(b"42", false, 0), "42");
        let uuid = [
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ];
        assert_eq!(
            format_value(&uuid, true, OID_UUID),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn result_preview_is_capped() {
        let mut t = connected();
        client(&mut t, &msg(b'Q', &cstr("SELECT n FROM big")), 0);
        let mut reply = row_description(&["n"]);
        for i in 0..(MAX_PREVIEW_ROWS + 5) {
            reply.extend(data_row(&[Some(&i.to_string())]));
        }
        reply.extend(msg(b'C', &cstr("SELECT 55")));
        reply.extend(ready());
        let done = server(&mut t, &reply, 1_000);
        let table = String::from_utf8(
            done[0]
                .response
                .as_ref()
                .expect("response")
                .body
                .clone()
                .expect("body"),
        )
        .expect("utf8");
        assert!(table.contains("… 5 more rows"));
        assert!(table.ends_with("(55 rows)"));
    }

    #[test]
    fn cleanup_drops_the_oldest_connections() {
        let mut t = PgTracker::new();
        for port in 0..5u16 {
            let mut f = flow();
            f.src_port = 40000 + port;
            t.process(&f, &startup(&[("user", "a")]), u64::from(port));
        }
        t.cleanup(2);
        assert_eq!(t.conns.len(), 2);
        assert!(t.conns.values().all(|c| c.started >= 3));
    }
}
