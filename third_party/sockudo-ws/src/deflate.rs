//! Per-Message Deflate Extension (RFC 7692)
//!
//! This module implements the permessage-deflate WebSocket extension,
//! which compresses message payloads using the DEFLATE algorithm.

use bytes::{Bytes, BytesMut};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

use crate::error::{Error, Result};

/// Trailer bytes that must be removed after compression and added before decompression
const DEFLATE_TRAILER: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

/// Default LZ77 window size (32KB = 2^15)
pub const DEFAULT_WINDOW_BITS: u8 = 15;

/// Minimum LZ77 window size (256 bytes = 2^8)
pub const MIN_WINDOW_BITS: u8 = 8;

/// Maximum LZ77 window size (32KB = 2^15)
pub const MAX_WINDOW_BITS: u8 = 15;

/// Configuration for permessage-deflate extension
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeflateConfig {
    /// Server's maximum LZ77 window bits (for compression when server, decompression when client)
    pub server_max_window_bits: u8,
    /// Client's maximum LZ77 window bits (for compression when client, decompression when server)
    pub client_max_window_bits: u8,
    /// If true, server must reset compression context after each message
    pub server_no_context_takeover: bool,
    /// If true, client must reset compression context after each message
    pub client_no_context_takeover: bool,
    /// Compression level (0-9, where 0 is no compression, 9 is max)
    pub compression_level: u32,
    /// Minimum message size to compress (smaller messages may not benefit)
    pub compression_threshold: usize,
}

impl Default for DeflateConfig {
    fn default() -> Self {
        Self {
            server_max_window_bits: DEFAULT_WINDOW_BITS,
            client_max_window_bits: DEFAULT_WINDOW_BITS,
            server_no_context_takeover: false,
            client_no_context_takeover: false,
            compression_level: 6,      // Default zlib compression level
            compression_threshold: 32, // Don't compress tiny messages
        }
    }
}

impl DeflateConfig {
    /// Create config optimized for low memory usage
    pub fn low_memory() -> Self {
        Self {
            server_max_window_bits: 10, // 1KB window
            client_max_window_bits: 10,
            server_no_context_takeover: true,
            client_no_context_takeover: true,
            compression_level: 1, // Fast compression
            compression_threshold: 64,
        }
    }

    /// Create config optimized for best compression
    pub fn best_compression() -> Self {
        Self {
            server_max_window_bits: MAX_WINDOW_BITS,
            client_max_window_bits: MAX_WINDOW_BITS,
            server_no_context_takeover: false,
            client_no_context_takeover: false,
            compression_level: 9,
            compression_threshold: 16,
        }
    }

    /// Parse extension parameters from handshake
    pub fn from_params(params: &[(&str, Option<&str>)]) -> Result<Self> {
        let mut config = Self::default();
        let mut seen = std::collections::HashSet::new();

        for (name, value) in params {
            if !seen.insert(*name) {
                return Err(Error::HandshakeFailed(
                    "duplicate permessage-deflate parameter",
                ));
            }
            match *name {
                "server_no_context_takeover" => {
                    if value.is_some() {
                        return Err(Error::HandshakeFailed(
                            "server_no_context_takeover must not have a value",
                        ));
                    }
                    config.server_no_context_takeover = true;
                }
                "client_no_context_takeover" => {
                    if value.is_some() {
                        return Err(Error::HandshakeFailed(
                            "client_no_context_takeover must not have a value",
                        ));
                    }
                    config.client_no_context_takeover = true;
                }
                "server_max_window_bits" => {
                    let v = value.ok_or(Error::HandshakeFailed(
                        "server_max_window_bits requires a value",
                    ))?;
                    config.server_max_window_bits = parse_window_bits(v)?;
                }
                "client_max_window_bits" => {
                    if let Some(v) = value {
                        config.client_max_window_bits = parse_window_bits(v)?;
                    }
                    // If no value, client just indicates support
                }
                _ => {
                    return Err(Error::HandshakeFailed(
                        "unknown permessage-deflate parameter",
                    ));
                }
            }
        }

        Ok(config)
    }

    /// Generate extension response header value for server
    pub fn to_response_header(&self) -> String {
        let mut parts = vec!["permessage-deflate".to_string()];

        if self.server_no_context_takeover {
            parts.push("server_no_context_takeover".to_string());
        }
        if self.client_no_context_takeover {
            parts.push("client_no_context_takeover".to_string());
        }
        if self.server_max_window_bits < MAX_WINDOW_BITS {
            parts.push(format!(
                "server_max_window_bits={}",
                self.server_max_window_bits
            ));
        }
        if self.client_max_window_bits < MAX_WINDOW_BITS {
            parts.push(format!(
                "client_max_window_bits={}",
                self.client_max_window_bits
            ));
        }

        parts.join("; ")
    }
}

/// Deflate compressor for outgoing messages
pub struct DeflateEncoder {
    compress: Compress,
    no_context_takeover: bool,
    #[allow(dead_code)]
    window_bits: u8,
    #[allow(dead_code)]
    compression_level: Compression,
    threshold: usize,
}

impl DeflateEncoder {
    /// Create a new encoder
    pub fn new(window_bits: u8, no_context_takeover: bool, level: u32, threshold: usize) -> Self {
        let compression_level = Compression::new(level);
        // Use the negotiated window_bits for compression
        // This ensures the compressed data can be decompressed by clients with smaller windows
        let compress = Compress::new_with_window_bits(compression_level, false, window_bits);

        Self {
            compress,
            no_context_takeover,
            window_bits,
            compression_level,
            threshold,
        }
    }

    /// Compress a message payload
    ///
    /// Returns None before touching the dictionary for messages below the
    /// threshold. After an attempted compression, a context-taking encoder must
    /// send the compressed result even if larger, keeping the peer in sync.
    pub fn compress(&mut self, data: &[u8]) -> Result<Option<Bytes>> {
        if data.len() < self.threshold {
            return Ok(None);
        }

        // Reset context if required
        if self.no_context_takeover {
            self.compress.reset();
        }

        // Estimate output size (compressed data is often smaller, but we need headroom)
        let max_output = data.len() + 64;
        let mut output = BytesMut::with_capacity(max_output);

        // Compress the data
        let mut total_in: usize = 0;
        let mut iterations = 0u32;

        loop {
            iterations += 1;
            if iterations > 100_000 {
                return Err(Error::Compression(
                    "compression took too many iterations".into(),
                ));
            }

            // Ensure we have space in output buffer
            let available = output.capacity() - output.len();
            if available == 0 {
                output.reserve(4096);
            }

            let input = &data[total_in..];
            let before_out = self.compress.total_out();
            let before_in = self.compress.total_in();

            // flate2's uninitialized-output API accepts spare capacity directly;
            // never reinterpret uninitialized memory as initialized byte values.
            let out_start = output.len();
            let spare = output.spare_capacity_mut();

            let status = self
                .compress
                .compress_uninit(input, spare, FlushCompress::Sync)
                .map_err(|e| Error::Compression(format!("deflate error: {}", e)))?;

            let consumed = (self.compress.total_in() - before_in) as usize;
            let produced = (self.compress.total_out() - before_out) as usize;

            total_in += consumed;

            // SAFETY: compress_uninit() initialized exactly `produced` bytes.
            // We're only extending the length by the number of bytes that were initialized.
            unsafe {
                output.set_len(out_start + produced);
            }

            match status {
                Status::Ok | Status::BufError => {
                    if total_in >= data.len() && output.len() < output.capacity() {
                        break;
                    }
                }
                Status::StreamEnd => break,
            }
        }

        // Per RFC 7692: Remove trailing 0x00 0x00 0xff 0xff
        if output.len() >= 4 && output.ends_with(&DEFLATE_TRAILER) {
            output.truncate(output.len() - 4);
        }

        // Only use compression if it actually reduces size
        if self.no_context_takeover && output.len() >= data.len() {
            return Ok(None);
        }

        Ok(Some(output.freeze()))
    }

    /// Reset the compression context (for no_context_takeover)
    pub fn reset(&mut self) {
        self.compress.reset();
    }
}

/// Deflate decompressor for incoming messages
pub struct DeflateDecoder {
    decompress: Decompress,
    no_context_takeover: bool,
    #[allow(dead_code)]
    window_bits: u8,
}

impl DeflateDecoder {
    /// Create a new decoder
    pub fn new(window_bits: u8, no_context_takeover: bool) -> Self {
        // Use raw deflate (no zlib header) with the negotiated window_bits
        // A 9-bit decoder also accepts a peer's valid 8-bit DEFLATE stream.
        // flate2 does not expose an 8-bit allocation and would otherwise panic.
        let decompress = Decompress::new_with_window_bits(false, window_bits.max(9));

        Self {
            decompress,
            no_context_takeover,
            window_bits,
        }
    }

    /// Decompress a message payload
    pub fn decompress(&mut self, data: &[u8], max_size: usize) -> Result<Bytes> {
        // Reset context if required
        if self.no_context_takeover {
            self.decompress.reset(false);
        }

        // Per RFC 7692: Append 0x00 0x00 0xff 0xff before decompressing
        let mut input = BytesMut::with_capacity(data.len() + 4);
        input.extend_from_slice(data);
        input.extend_from_slice(&DEFLATE_TRAILER);

        // Start with reasonable output buffer (at least 1KB or 4x input)
        let allocation_limit = max_size.saturating_add(1);
        let initial_cap = std::cmp::max(1024, data.len().saturating_mul(4)).min(allocation_limit);
        let mut output = BytesMut::with_capacity(initial_cap);
        let mut total_in: usize = 0;
        let mut iterations = 0u32;

        loop {
            iterations += 1;
            // Safety check to prevent infinite loops
            if iterations > 100_000 {
                return Err(Error::Compression(
                    "decompression took too many iterations".into(),
                ));
            }

            // Check size limit
            if output.len() > max_size {
                return Err(Error::MessageTooLarge);
            }

            // Ensure we have space in output buffer
            let available = output.capacity() - output.len();
            if available == 0 {
                if output.len() >= allocation_limit {
                    return Err(Error::MessageTooLarge);
                }
                // At least double or add 4KB, whichever is larger
                let additional =
                    std::cmp::max(output.capacity(), 4096).min(allocation_limit - output.len());
                output.reserve(additional);
            }

            let before_out = self.decompress.total_out();
            let before_in = self.decompress.total_in();

            // Restrict the uninitialized output slice before asking flate2 to
            // initialize it, keeping the decompressed limit independently bound.
            let out_start = output.len();
            let spare = output.spare_capacity_mut();
            let writable = spare.len().min(allocation_limit - out_start);
            let spare = &mut spare[..writable];

            let status = self
                .decompress
                .decompress_uninit(&input[total_in..], spare, FlushDecompress::Sync)
                .map_err(|e| Error::Compression(format!("inflate error: {}", e)))?;

            let consumed = (self.decompress.total_in() - before_in) as usize;
            let produced = (self.decompress.total_out() - before_out) as usize;

            total_in += consumed;

            // SAFETY: decompress_uninit() initialized exactly `produced` bytes.
            // We're only extending the length by the number of bytes that were initialized.
            unsafe {
                output.set_len(out_start + produced);
            }

            if output.len() > max_size {
                return Err(Error::MessageTooLarge);
            }
            if consumed == 0 && produced == 0 {
                if total_in == input.len() {
                    break;
                }
                return Err(Error::Compression("inflate made no progress".into()));
            }

            match status {
                Status::Ok => {
                    if total_in >= input.len() {
                        break;
                    }
                }
                Status::StreamEnd => break,
                Status::BufError => {
                    // Need more output space - will be handled at top of loop
                }
            }
        }

        Ok(output.freeze())
    }

    /// Reset the decompression context (for no_context_takeover)
    pub fn reset(&mut self) {
        self.decompress.reset(false);
    }
}

/// Combined compressor/decompressor context for a WebSocket connection
pub struct DeflateContext {
    /// Encoder for outgoing messages
    pub encoder: DeflateEncoder,
    /// Decoder for incoming messages
    pub decoder: DeflateDecoder,
    /// Configuration
    pub config: DeflateConfig,
}

impl DeflateContext {
    /// Create context for server role
    pub fn server(config: DeflateConfig) -> Self {
        let encoder = DeflateEncoder::new(
            config.server_max_window_bits,
            config.server_no_context_takeover,
            config.compression_level,
            config.compression_threshold,
        );
        let decoder = DeflateDecoder::new(
            config.client_max_window_bits,
            config.client_no_context_takeover,
        );

        Self {
            encoder,
            decoder,
            config,
        }
    }

    /// Create context for client role
    pub fn client(config: DeflateConfig) -> Self {
        let encoder = DeflateEncoder::new(
            config.client_max_window_bits,
            config.client_no_context_takeover,
            config.compression_level,
            config.compression_threshold,
        );
        let decoder = DeflateDecoder::new(
            config.server_max_window_bits,
            config.server_no_context_takeover,
        );

        Self {
            encoder,
            decoder,
            config,
        }
    }

    /// Compress a message if beneficial
    pub fn compress(&mut self, data: &[u8]) -> Result<Option<Bytes>> {
        self.encoder.compress(data)
    }

    /// Decompress a message
    pub fn decompress(&mut self, data: &[u8], max_size: usize) -> Result<Bytes> {
        self.decoder.decompress(data, max_size)
    }
}

fn parse_window_bits(value: &str) -> Result<u8> {
    if !matches!(value, "8" | "9" | "10" | "11" | "12" | "13" | "14" | "15") {
        return Err(Error::HandshakeFailed(
            "invalid permessage-deflate window bits",
        ));
    }
    Ok(value.parse().expect("validated decimal window bits"))
}

// Extension separators inside HTTP quoted strings do not delimit offers or
// parameters. An unterminated quote invalidates the header rather than making
// an arbitrary suffix into a separately negotiated extension.
fn split_extensions(value: &str, delimiter: u8) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (index, byte) in value.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if quoted && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if !quoted && byte == delimiter {
            parts.push(value[start..index].trim());
            start = index + 1;
        }
    }
    if quoted || escaped {
        return None;
    }
    parts.push(value[start..].trim());
    Some(parts)
}

/// Parse a single permessage-deflate offer. Header alternatives are handled by
/// `negotiate_deflate`, never folded into the first offer's parameters.
pub fn parse_deflate_offer(value: &str) -> Option<Vec<(&str, Option<&str>)>> {
    let parts = split_extensions(value, b';')?;
    if *parts.first()? != "permessage-deflate" {
        return None;
    }
    let mut params = Vec::new();
    for part in &parts[1..] {
        if part.is_empty() || part.contains(',') {
            return None;
        }
        if let Some((name, value)) = part.split_once('=') {
            let name = name.trim();
            let value = value.trim();
            let value = if value.starts_with('"') {
                value.strip_prefix('"')?.strip_suffix('"')?
            } else {
                value
            };
            if name.is_empty() || value.is_empty() || value.contains(['"', '\\']) {
                return None;
            }
            params.push((name, Some(value)));
        } else {
            params.push((*part, None));
        }
    }
    Some(params)
}

/// Negotiate the first supported offer from an RFC 6455 extension list.
/// The returned configuration is exactly what the response advertises.
pub fn negotiate_deflate(
    value: &str,
    preferred: &DeflateConfig,
) -> Option<(String, DeflateConfig)> {
    if !(9..=15).contains(&preferred.server_max_window_bits)
        || !(8..=15).contains(&preferred.client_max_window_bits)
        || preferred.compression_level > 9
    {
        return None;
    }
    for offer in split_extensions(value, b',')? {
        let Some(params) = parse_deflate_offer(offer) else {
            continue;
        };
        let Ok(client) = DeflateConfig::from_params(&params) else {
            continue;
        };
        let has = |name: &str| params.iter().any(|(key, _)| *key == name);
        let mut agreed = preferred.clone();
        agreed.server_max_window_bits = preferred
            .server_max_window_bits
            .min(client.server_max_window_bits);
        // flate2 cannot encode with an 8-bit window. Decline that offer, leaving
        // the connection uncompressed or allowing a later compatible offer.
        if agreed.server_max_window_bits < 9 {
            continue;
        }
        agreed.server_no_context_takeover |= client.server_no_context_takeover;
        agreed.client_no_context_takeover |= client.client_no_context_takeover;
        agreed.client_max_window_bits = if has("client_max_window_bits") {
            preferred
                .client_max_window_bits
                .min(client.client_max_window_bits)
        } else {
            // MUST NOT advertise client_max_window_bits unless it was offered.
            MAX_WINDOW_BITS
        };
        let mut response = agreed.to_response_header();
        // Explicit 15-bit limits must also be echoed when offered; omitting
        // them otherwise means the normal, implicit RFC 7692 32 KiB default.
        if has("server_max_window_bits") && agreed.server_max_window_bits == MAX_WINDOW_BITS {
            response.push_str("; server_max_window_bits=15");
        }
        if has("client_max_window_bits") && agreed.client_max_window_bits == MAX_WINDOW_BITS {
            response.push_str("; client_max_window_bits=15");
        }
        return Some((response, agreed));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takeover_keeps_dictionary_in_sync_when_compression_expands() {
        let config = DeflateConfig {
            compression_threshold: 0,
            ..Default::default()
        };
        let mut server = DeflateContext::server(config.clone());
        let mut client = DeflateContext::client(config);
        let mut seed = 0x9e37_79b9u32;
        let noise: Vec<u8> = (0..512)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let first = server
            .compress(&noise)
            .unwrap()
            .expect("mutated context must be sent");
        assert!(
            first.len() >= noise.len(),
            "fixture must exercise the expanding branch"
        );
        assert_eq!(client.decompress(&first, 4096).unwrap().as_ref(), noise);
        let repeated = noise.repeat(4);
        let second = server.compress(&repeated).unwrap().unwrap();
        assert_eq!(client.decompress(&second, 4096).unwrap().as_ref(), repeated);
    }

    #[test]
    fn no_takeover_can_skip_expanding_message_without_affecting_next_message() {
        let config = DeflateConfig {
            server_no_context_takeover: true,
            compression_threshold: 0,
            ..Default::default()
        };
        let mut server = DeflateContext::server(config.clone());
        let mut client = DeflateContext::client(config);
        assert!(server.compress(b"1").unwrap().is_none());
        let next = b"queue revision apparatus order controls ".repeat(20);
        let payload = server.compress(&next).unwrap().unwrap();
        assert_eq!(client.decompress(&payload, 4096).unwrap().as_ref(), next);
    }

    #[test]
    fn negotiation_respects_each_direction_and_rejects_unsupported_windows() {
        let (header, agreed) = negotiate_deflate(
            "permessage-deflate; server_max_window_bits=10; client_max_window_bits=12; server_no_context_takeover",
            &DeflateConfig::default(),
        ).unwrap();
        assert_eq!(agreed.server_max_window_bits, 10);
        assert_eq!(agreed.client_max_window_bits, 12);
        assert!(agreed.server_no_context_takeover);
        assert!(header.contains("server_max_window_bits=10"));
        assert!(header.contains("client_max_window_bits=12"));
        assert!(
            negotiate_deflate(
                "permessage-deflate; server_max_window_bits=8",
                &DeflateConfig::default()
            )
            .is_none()
        );
    }

    #[test]
    fn negotiation_does_not_advertise_unoffered_client_window_constraint() {
        let preferred = DeflateConfig::low_memory();
        let (header, agreed) = negotiate_deflate("permessage-deflate", &preferred).unwrap();
        assert_eq!(agreed.client_max_window_bits, 15);
        assert!(!header.contains("client_max_window_bits"));
    }

    #[test]
    fn negotiation_handles_alternative_offers_and_quoted_parameters() {
        let (header, agreed) = negotiate_deflate(
            "other-extension; value=\"a,b;c\", permessage-deflate; unknown=1, permessage-deflate; server_max_window_bits=\"11\"; client_max_window_bits",
            &DeflateConfig::default(),
        ).unwrap();
        assert_eq!(agreed.server_max_window_bits, 11);
        assert!(header.contains("client_max_window_bits=15"));
        assert!(
            negotiate_deflate(
                "permessage-deflate; server_max_window_bits=8, permessage-deflate",
                &DeflateConfig::default()
            )
            .is_some()
        );
    }

    #[test]
    fn malformed_parameters_and_duplicates_are_declined() {
        for offer in [
            "permessage-deflate; server_max_window_bits",
            "permessage-deflate; server_max_window_bits=09",
            "permessage-deflate; server_max_window_bits=+9",
            "permessage-deflate; server_max_window_bits=16",
            "permessage-deflate; server_no_context_takeover=1",
            "permessage-deflate; client_max_window_bits=10; client_max_window_bits=12",
            "permessage-deflate; client_no_context_takeover; client_no_context_takeover",
            "permessage-deflate; client_max_window_bits=\"12",
            "permessage-deflate;",
        ] {
            assert!(
                negotiate_deflate(offer, &DeflateConfig::default()).is_none(),
                "accepted {offer}"
            );
        }
    }

    #[test]
    fn client_no_context_takeover_does_not_force_server_no_context_takeover() {
        let config = DeflateConfig::from_params(&[("client_no_context_takeover", None)]).unwrap();
        assert!(config.client_no_context_takeover);
        assert!(!config.server_no_context_takeover);
    }

    #[test]
    fn decompressed_size_limit_is_checked_before_returning_a_single_chunk() {
        let mut encoder = DeflateEncoder::new(15, true, 3, 0);
        let message = b"large queue snapshot ".repeat(200);
        let encoded = encoder.compress(&message).unwrap().unwrap();
        assert!(matches!(
            DeflateDecoder::new(15, true).decompress(&encoded, 100),
            Err(Error::MessageTooLarge)
        ));
        assert_eq!(
            DeflateDecoder::new(15, true)
                .decompress(&encoded, message.len())
                .unwrap()
                .as_ref(),
            message
        );
    }

    #[test]
    fn multi_megabyte_incompressible_message_finishes_the_deflate_flush() {
        let mut seed = 0x1234_5678u32;
        let message: Vec<u8> = (0..(2 * 1024 * 1024))
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let mut encoder = DeflateEncoder::new(15, false, 3, 0);
        let encoded = encoder.compress(&message).unwrap().unwrap();
        assert_eq!(
            DeflateDecoder::new(15, false)
                .decompress(&encoded, message.len())
                .unwrap()
                .as_ref(),
            message
        );
    }

    #[test]
    fn test_compress_decompress() {
        let config = DeflateConfig::default();
        let mut ctx = DeflateContext::server(config);

        let original = b"Hello, World! This is a test message that should be compressed.";

        // Compress
        let compressed = ctx.compress(original).unwrap();
        assert!(compressed.is_some());
        let compressed = compressed.unwrap();
        assert!(compressed.len() < original.len());

        // Decompress
        let decompressed = ctx.decompress(&compressed, 1024).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[test]
    fn test_small_message_not_compressed() {
        let config = DeflateConfig {
            compression_threshold: 100,
            ..Default::default()
        };
        let mut ctx = DeflateContext::server(config);

        let small = b"tiny";
        let result = ctx.compress(small).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_context_takeover() {
        let config = DeflateConfig {
            server_no_context_takeover: false,
            compression_threshold: 0,
            ..Default::default()
        };
        let mut ctx = DeflateContext::server(config);

        let msg = b"Hello, World! Hello, World! Hello, World!";

        // First compression
        let first = ctx.compress(msg).unwrap().unwrap();

        // Second compression should benefit from context
        let second = ctx.compress(msg).unwrap().unwrap();

        // With context takeover, second should be smaller or equal
        // (references previous data in LZ77 window)
        assert!(second.len() <= first.len());
    }

    #[test]
    fn test_no_context_takeover() {
        let config = DeflateConfig {
            server_no_context_takeover: true,
            compression_threshold: 0,
            ..Default::default()
        };
        let mut ctx = DeflateContext::server(config);

        let msg = b"Hello, World! Hello, World! Hello, World!";

        // Both compressions should produce same output
        let first = ctx.compress(msg).unwrap().unwrap();
        let second = ctx.compress(msg).unwrap().unwrap();

        assert_eq!(first.len(), second.len());
    }

    #[test]
    fn test_parse_deflate_offer() {
        // Simple offer
        let params = parse_deflate_offer("permessage-deflate").unwrap();
        assert!(params.is_empty());

        // With parameters
        let params = parse_deflate_offer(
            "permessage-deflate; server_no_context_takeover; server_max_window_bits=10",
        )
        .unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], ("server_no_context_takeover", None));
        assert_eq!(params[1], ("server_max_window_bits", Some("10")));

        // Not a deflate offer
        assert!(parse_deflate_offer("some-other-extension").is_none());
    }

    #[test]
    fn test_config_from_params() {
        let params = vec![
            ("server_no_context_takeover", None),
            ("client_max_window_bits", Some("12")),
        ];

        let config = DeflateConfig::from_params(&params).unwrap();
        assert!(config.server_no_context_takeover);
        assert!(!config.client_no_context_takeover);
        assert_eq!(config.client_max_window_bits, 12);
        assert_eq!(config.server_max_window_bits, DEFAULT_WINDOW_BITS);
    }

    #[test]
    fn test_response_header() {
        let config = DeflateConfig {
            server_no_context_takeover: true,
            server_max_window_bits: 12,
            ..Default::default()
        };

        let header = config.to_response_header();
        assert!(header.contains("permessage-deflate"));
        assert!(header.contains("server_no_context_takeover"));
        assert!(header.contains("server_max_window_bits=12"));
    }
}
