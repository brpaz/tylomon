//! Schema-less Protocol Buffers decoding, in the spirit of `protoc --decode_raw`.
//!
//! Without the `.proto` files only wire types are known, so field numbers are shown and
//! values are guessed: length-delimited fields are printed as text when they look like text,
//! as a nested message when they parse as one, and as hex otherwise.

use std::fmt::Write;

const MAX_DEPTH: usize = 8;
const MAX_FIELD_NUMBER: u64 = (1 << 29) - 1;
const MAX_TEXT_CHARS: usize = 300;
const MAX_HEX_BYTES: usize = 32;

#[derive(Debug, PartialEq)]
enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Fixed32(u32),
    Bytes(&'a [u8]),
}

struct Field<'a> {
    number: u64,
    value: Value<'a>,
}

/// Parse `bytes` as one message. `None` if it is not valid wire format or is empty.
fn parse(bytes: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut fields = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (tag, used) = read_varint(&bytes[pos..])?;
        pos += used;
        let number = tag >> 3;
        if number == 0 || number > MAX_FIELD_NUMBER {
            return None;
        }
        let value = match tag & 7 {
            0 => {
                let (v, used) = read_varint(&bytes[pos..])?;
                pos += used;
                Value::Varint(v)
            }
            1 => {
                let raw = bytes.get(pos..pos + 8)?;
                pos += 8;
                Value::Fixed64(u64::from_le_bytes(raw.try_into().ok()?))
            }
            2 => {
                let (len, used) = read_varint(&bytes[pos..])?;
                pos += used;
                let end = pos.checked_add(usize::try_from(len).ok()?)?;
                let raw = bytes.get(pos..end)?;
                pos = end;
                Value::Bytes(raw)
            }
            5 => {
                let raw = bytes.get(pos..pos + 4)?;
                pos += 4;
                Value::Fixed32(u32::from_le_bytes(raw.try_into().ok()?))
            }
            // Groups are deprecated and 3 or 4 almost always means this is not a message.
            _ => return None,
        };
        fields.push(Field { number, value });
    }
    (!fields.is_empty()).then_some(fields)
}

fn read_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (index, &byte) in bytes.iter().take(10).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

fn looks_like_text(bytes: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.chars()
        .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .then_some(text)
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for (count, c) in text.chars().enumerate() {
        if count >= MAX_TEXT_CHARS {
            out.push('…');
            break;
        }
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn hex(bytes: &[u8]) -> String {
    let shown: String = bytes
        .iter()
        .take(MAX_HEX_BYTES)
        .map(|b| format!("{b:02x}"))
        .collect();
    let more = if bytes.len() > MAX_HEX_BYTES {
        "…"
    } else {
        ""
    };
    format!("bytes[{}] {shown}{more}", bytes.len())
}

fn fixed64(raw: u64) -> String {
    let float = f64::from_bits(raw);
    if float.is_finite() && (float == 0.0 || (1e-6..1e12).contains(&float.abs())) {
        format!("{float} (0x{raw:016x})")
    } else {
        format!("0x{raw:016x}")
    }
}

fn fixed32(raw: u32) -> String {
    let float = f32::from_bits(raw);
    if float.is_finite() && (float == 0.0 || (1e-4..1e9).contains(&float.abs())) {
        format!("{float} (0x{raw:08x})")
    } else {
        format!("0x{raw:08x}")
    }
}

fn render(fields: &[Field], depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    for field in fields {
        let number = field.number;
        match &field.value {
            Value::Varint(v) => {
                let _ = writeln!(out, "{indent}{number}: {v}");
            }
            Value::Fixed64(v) => {
                let _ = writeln!(out, "{indent}{number}: {}", fixed64(*v));
            }
            Value::Fixed32(v) => {
                let _ = writeln!(out, "{indent}{number}: {}", fixed32(*v));
            }
            Value::Bytes(bytes) => {
                if bytes.is_empty() {
                    let _ = writeln!(out, "{indent}{number}: \"\"");
                } else if let Some(text) = looks_like_text(bytes) {
                    let _ = writeln!(out, "{indent}{number}: {}", quote(text));
                } else if let Some(nested) = parse(bytes).filter(|_| depth < MAX_DEPTH) {
                    let _ = writeln!(out, "{indent}{number} {{");
                    render(&nested, depth + 1, out);
                    let _ = writeln!(out, "{indent}}}");
                } else {
                    let _ = writeln!(out, "{indent}{number}: {}", hex(bytes));
                }
            }
        }
    }
}

/// Render a message as indented `field: value` lines. Data that is not a valid message is
/// shown as hex.
pub fn decode_raw(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "(empty message)".to_string();
    }
    let Some(fields) = parse(bytes) else {
        return hex(bytes);
    };
    let mut out = String::new();
    render(&fields, 0, &mut out);
    out.truncate(out.trim_end().len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn field_varint(number: u64, value: u64) -> Vec<u8> {
        [varint(number << 3), varint(value)].concat()
    }

    fn field_bytes(number: u64, value: &[u8]) -> Vec<u8> {
        [
            varint(number << 3 | 2),
            varint(value.len() as u64),
            value.to_vec(),
        ]
        .concat()
    }

    #[test]
    fn varints_and_strings() {
        let msg = [field_bytes(1, b"hello"), field_varint(2, 300)].concat();
        assert_eq!(decode_raw(&msg), "1: \"hello\"\n2: 300");
    }

    #[test]
    fn nested_messages_are_indented() {
        let inner = [field_varint(1, 7), field_bytes(2, b"x")].concat();
        let msg = [field_bytes(3, &inner), field_varint(4, 1)].concat();
        assert_eq!(decode_raw(&msg), "3 {\n  1: 7\n  2: \"x\"\n}\n4: 1");
    }

    #[test]
    fn deeply_nested_messages() {
        let level3 = field_varint(1, 9);
        let level2 = field_bytes(1, &level3);
        let level1 = field_bytes(1, &level2);
        let rendered = decode_raw(&field_bytes(1, &level1));
        assert!(rendered.contains("      1: 9"), "{rendered}");
    }

    #[test]
    fn text_wins_over_a_coincidental_message() {
        // "(" is a valid tag byte, but printable text should read as text.
        assert_eq!(decode_raw(&field_bytes(1, b"(abc)")), "1: \"(abc)\"");
    }

    #[test]
    fn binary_data_that_is_not_a_message_is_hex() {
        let msg = field_bytes(1, &[0xff, 0xfe, 0x00, 0x80]);
        assert_eq!(decode_raw(&msg), "1: bytes[4] fffe0080");
    }

    #[test]
    fn fixed_width_values() {
        let mut msg = varint(1 << 3 | 1);
        msg.extend(1.5f64.to_le_bytes());
        msg.extend(varint(2 << 3 | 5));
        msg.extend(7u32.to_le_bytes());
        let rendered = decode_raw(&msg);
        assert_eq!(rendered, "1: 1.5 (0x3ff8000000000000)\n2: 0x00000007");
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(
            decode_raw(&field_bytes(1, b"say \"hi\"\n")),
            "1: \"say \\\"hi\\\"\\n\""
        );
    }

    #[test]
    fn empty_bytes_field() {
        assert_eq!(decode_raw(&field_bytes(1, b"")), "1: \"\"");
    }

    #[test]
    fn invalid_wire_format_falls_back_to_hex() {
        assert_eq!(decode_raw(&[0x0b, 0x01]), "bytes[2] 0b01");
        // Truncated length-delimited field.
        assert_eq!(decode_raw(&[0x0a, 0x05, b'a']), "bytes[3] 0a0561");
        // Field number zero.
        assert_eq!(decode_raw(&[0x00, 0x01]), "bytes[2] 0001");
    }

    #[test]
    fn empty_message_is_labelled() {
        assert_eq!(decode_raw(&[]), "(empty message)");
    }

    #[test]
    fn long_text_is_truncated() {
        let long = "a".repeat(MAX_TEXT_CHARS + 50);
        let rendered = decode_raw(&field_bytes(1, long.as_bytes()));
        assert!(rendered.ends_with("…\""));
        assert!(rendered.chars().count() < MAX_TEXT_CHARS + 20);
    }

    #[test]
    fn varint_overflowing_ten_bytes_is_rejected() {
        assert_eq!(read_varint(&[0xff; 11]), None);
        assert_eq!(read_varint(&[0x96, 0x01]), Some((150, 2)));
    }
}
