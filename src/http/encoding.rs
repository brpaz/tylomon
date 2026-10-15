use std::io::Read;

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};

/// Upper bound on decoded output, guarding against decompression bombs.
const MAX_DECODED_BYTES: u64 = 16 * 1024 * 1024;

const BROTLI_BUFFER_SIZE: usize = 4096;

/// Decode a body according to its `Content-Encoding` header.
///
/// Returns `None` when the encoding is absent, unsupported or the data cannot be decoded at
/// all, so the caller keeps the raw bytes. A truncated stream (the capture saw only part of
/// the body) yields whatever decoded before the cut.
pub fn decode_body(headers: &[(String, String)], body: &[u8]) -> Option<Vec<u8>> {
    let encoding = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-encoding"))
        .map(|(_, v)| v.trim().to_ascii_lowercase())?;

    match encoding.as_str() {
        "gzip" | "x-gzip" => read_limited(GzDecoder::new(body)),
        "br" => read_limited(brotli::Decompressor::new(body, BROTLI_BUFFER_SIZE)),
        // HTTP "deflate" is zlib-wrapped, but some servers send a raw deflate stream.
        "deflate" => {
            read_limited(ZlibDecoder::new(body)).or_else(|| read_limited(DeflateDecoder::new(body)))
        }
        _ => None,
    }
}

fn read_limited(reader: impl Read) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    // Errors after some output (truncated input) are expected for partially captured bodies.
    let _ = reader.take(MAX_DECODED_BYTES).read_to_end(&mut out);
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};

    use super::*;

    fn headers(encoding: &str) -> Vec<(String, String)> {
        vec![("Content-Encoding".into(), encoding.into())]
    }

    const TEXT: &[u8] = b"{\"hello\":\"world\"} repeated repeated repeated repeated";

    #[test]
    fn decodes_gzip() {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(TEXT).expect("write");
        let body = enc.finish().expect("finish");
        assert_eq!(decode_body(&headers("gzip"), &body).as_deref(), Some(TEXT));
    }

    #[test]
    fn decodes_zlib_and_raw_deflate() {
        let mut zlib = ZlibEncoder::new(Vec::new(), Compression::default());
        zlib.write_all(TEXT).expect("write");
        let zlib = zlib.finish().expect("finish");
        assert_eq!(
            decode_body(&headers("deflate"), &zlib).as_deref(),
            Some(TEXT)
        );

        let mut raw = DeflateEncoder::new(Vec::new(), Compression::default());
        raw.write_all(TEXT).expect("write");
        let raw = raw.finish().expect("finish");
        assert_eq!(
            decode_body(&headers("deflate"), &raw).as_deref(),
            Some(TEXT)
        );
    }

    #[test]
    fn decodes_brotli() {
        let mut body = Vec::new();
        {
            let mut enc = brotli::CompressorWriter::new(&mut body, 4096, 5, 22);
            enc.write_all(TEXT).expect("write");
        }
        assert_eq!(decode_body(&headers("br"), &body).as_deref(), Some(TEXT));
    }

    #[test]
    fn header_name_and_value_are_case_insensitive() {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(TEXT).expect("write");
        let body = enc.finish().expect("finish");
        let headers = vec![("content-encoding".to_string(), " GZIP ".to_string())];
        assert_eq!(decode_body(&headers, &body).as_deref(), Some(TEXT));
    }

    #[test]
    fn truncated_stream_yields_partial_output() {
        let mut enc = GzEncoder::new(Vec::new(), Compression::none());
        enc.write_all(TEXT).expect("write");
        let body = enc.finish().expect("finish");
        let cut = &body[..body.len() - 12];
        let decoded = decode_body(&headers("gzip"), cut).expect("partial");
        assert!(!decoded.is_empty());
        assert!(TEXT.starts_with(&decoded));
    }

    #[test]
    fn unsupported_missing_or_invalid_encoding_is_none() {
        assert_eq!(decode_body(&[], TEXT), None);
        assert_eq!(decode_body(&headers("zstd"), TEXT), None);
        assert_eq!(decode_body(&headers("identity"), TEXT), None);
        assert_eq!(decode_body(&headers("gzip"), b"not gzip data"), None);
    }

    #[test]
    fn output_is_capped() {
        let big = vec![b'a'; (MAX_DECODED_BYTES as usize) + 1024];
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(&big).expect("write");
        let body = enc.finish().expect("finish");
        let decoded = decode_body(&headers("gzip"), &body).expect("decoded");
        assert_eq!(decoded.len() as u64, MAX_DECODED_BYTES);
    }
}
