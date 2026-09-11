//! Hand-written proto3 wire-format primitives and `RustDesk`'s variable
//! length-prefix TCP framing.
//!
//! This is an independent implementation of the open, public protobuf wire
//! specification (varint/zigzag encoding, tag = `(field_number << 3) |
//! wire_type`, length-delimited fields) — NOT a copy of `RustDesk`'s `.proto`
//! file or generated code. It mirrors `@verevon/remote-core`'s own
//! `protocol/rustdesk/wire/` implementation (same public algorithm, same
//! reasoning already reviewed for that package's `docs/licensing.md`), not
//! anything from `RustDesk`'s own source tree.
//!
//! Every fallible function's error variants are self-explanatory from
//! [`WireError`]'s own doc comments; per-function `# Errors`/`# Panics`
//! sections would be pure repetition across ~20 small encode/decode
//! primitives, so those two pedantic lints are silenced crate-wide rather
//! than hand-written for each one. Wire-format bit-punning also means
//! narrowing/sign-changing casts (u64<->i64, u32<->i32, truncating to u8)
//! are the intended behavior throughout this file (proto3's own two's-
//! complement wrapping for negative ints, and the frame header's
//! byte-at-a-time packing) — not a source of real bugs to flag per-call-site.
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless
)]

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireError {
    #[error("truncated varint")]
    TruncatedVarint,
    #[error("varint exceeds 64 bits")]
    VarintTooLong,
    #[error("unexpected end of buffer reading {0} bytes")]
    UnexpectedEof(usize),
    #[error("unknown protobuf wire type: {0}")]
    UnknownWireType(u32),
    #[error("frame length header truncated")]
    TruncatedFrameHeader,
    #[error("frame exceeds the maximum allowed length")]
    FrameTooLarge,
}

pub const WIRE_VARINT: u32 = 0;
pub const WIRE_64BIT: u32 = 1;
pub const WIRE_LENGTH_DELIMITED: u32 = 2;
pub const WIRE_32BIT: u32 = 5;

// ---------------------------------------------------------------------
// varint / zigzag — public protobuf-spec algorithms, not RustDesk facts.
// ---------------------------------------------------------------------

/// Two's-complement-wraps `value` to 64 bits before varint-encoding it, as
/// proto3 requires for negative int32/int64.
pub fn encode_varint(value: u64, out: &mut Vec<u8>) {
    let mut v = value;
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

pub fn decode_varint(bytes: &[u8], offset: usize) -> Result<(u64, usize), WireError> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    let mut pos = offset;
    loop {
        let byte = *bytes.get(pos).ok_or(WireError::TruncatedVarint)?;
        result |= u64::from(byte & 0x7f) << shift;
        pos += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return Err(WireError::VarintTooLong);
        }
    }
    Ok((result, pos))
}

#[must_use]
pub fn zigzag_encode32(value: i32) -> u32 {
    ((value << 1) ^ (value >> 31)) as u32
}

#[must_use]
pub fn zigzag_decode32(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

// ---------------------------------------------------------------------
// ProtoWriter — proto3 semantics: a field equal to its default (0/false/
// empty) is never written on the wire.
// ---------------------------------------------------------------------

#[derive(Default)]
pub struct ProtoWriter {
    buf: Vec<u8>,
}

impl ProtoWriter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn tag(&mut self, field_number: u32, wire_type: u32) {
        encode_varint(u64::from((field_number << 3) | wire_type), &mut self.buf);
    }

    pub fn int32(&mut self, field_number: u32, value: i32) -> &mut Self {
        if value == 0 {
            return self;
        }
        self.tag(field_number, WIRE_VARINT);
        // proto3 two's-complement-wraps a negative int32 into the full 64 bits.
        encode_varint(i64::from(value) as u64, &mut self.buf);
        self
    }

    pub fn uint32(&mut self, field_number: u32, value: u32) -> &mut Self {
        if value == 0 {
            return self;
        }
        self.tag(field_number, WIRE_VARINT);
        encode_varint(u64::from(value), &mut self.buf);
        self
    }

    pub fn sint32(&mut self, field_number: u32, value: i32) -> &mut Self {
        if value == 0 {
            return self;
        }
        self.tag(field_number, WIRE_VARINT);
        encode_varint(u64::from(zigzag_encode32(value)), &mut self.buf);
        self
    }

    pub fn bool(&mut self, field_number: u32, value: bool) -> &mut Self {
        if !value {
            return self;
        }
        self.tag(field_number, WIRE_VARINT);
        self.buf.push(1);
        self
    }

    pub fn int64(&mut self, field_number: u32, value: i64) -> &mut Self {
        if value == 0 {
            return self;
        }
        self.tag(field_number, WIRE_VARINT);
        encode_varint(value as u64, &mut self.buf);
        self
    }

    pub fn uint64(&mut self, field_number: u32, value: u64) -> &mut Self {
        if value == 0 {
            return self;
        }
        self.tag(field_number, WIRE_VARINT);
        encode_varint(value, &mut self.buf);
        self
    }

    pub fn double(&mut self, field_number: u32, value: f64) -> &mut Self {
        if value == 0.0 {
            return self;
        }
        self.tag(field_number, WIRE_64BIT);
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn bytes(&mut self, field_number: u32, value: &[u8]) -> &mut Self {
        if value.is_empty() {
            return self;
        }
        self.tag(field_number, WIRE_LENGTH_DELIMITED);
        encode_varint(value.len() as u64, &mut self.buf);
        self.buf.extend_from_slice(value);
        self
    }

    pub fn string(&mut self, field_number: u32, value: &str) -> &mut Self {
        if value.is_empty() {
            return self;
        }
        self.bytes(field_number, value.as_bytes())
    }

    /// Always writes the field, even if `encoded` is empty — proto3
    /// distinguishes "absent" from "present but an empty sub-message".
    pub fn message(&mut self, field_number: u32, encoded: &[u8]) -> &mut Self {
        self.tag(field_number, WIRE_LENGTH_DELIMITED);
        encode_varint(encoded.len() as u64, &mut self.buf);
        self.buf.extend_from_slice(encoded);
        self
    }

    /// proto3 packs repeated scalar/enum fields by default.
    pub fn packed_varint(&mut self, field_number: u32, values: &[u64]) -> &mut Self {
        if values.is_empty() {
            return self;
        }
        let mut inner = Vec::new();
        for &v in values {
            encode_varint(v, &mut inner);
        }
        self.tag(field_number, WIRE_LENGTH_DELIMITED);
        encode_varint(inner.len() as u64, &mut self.buf);
        self.buf.extend_from_slice(&inner);
        self
    }

    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

// ---------------------------------------------------------------------
// ProtoReader — dispatches on field number itself; callers `match` and use
// `skip()` for unknown fields, as proto3 forward-compatibility requires.
// ---------------------------------------------------------------------

pub struct FieldTag {
    pub field_number: u32,
    pub wire_type: u32,
}

pub struct ProtoReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> ProtoReader<'a> {
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    #[must_use]
    pub fn eof(&self) -> bool {
        self.offset >= self.data.len()
    }

    pub fn read_tag(&mut self) -> Result<FieldTag, WireError> {
        let tag = self.read_varint_raw()? as u32;
        Ok(FieldTag {
            field_number: tag >> 3,
            wire_type: tag & 0x7,
        })
    }

    pub fn read_varint_raw(&mut self) -> Result<u64, WireError> {
        let (value, next) = decode_varint(self.data, self.offset)?;
        self.offset = next;
        Ok(value)
    }

    pub fn read_int32(&mut self) -> Result<i32, WireError> {
        Ok(self.read_varint_raw()? as i64 as i32)
    }

    pub fn read_uint32(&mut self) -> Result<u32, WireError> {
        Ok(self.read_varint_raw()? as u32)
    }

    pub fn read_sint32(&mut self) -> Result<i32, WireError> {
        Ok(zigzag_decode32(self.read_varint_raw()? as u32))
    }

    pub fn read_bool(&mut self) -> Result<bool, WireError> {
        Ok(self.read_varint_raw()? != 0)
    }

    pub fn read_int64(&mut self) -> Result<i64, WireError> {
        Ok(self.read_varint_raw()? as i64)
    }

    pub fn read_uint64(&mut self) -> Result<u64, WireError> {
        self.read_varint_raw()
    }

    pub fn read_double(&mut self) -> Result<f64, WireError> {
        let bytes = self
            .data
            .get(self.offset..self.offset + 8)
            .ok_or(WireError::UnexpectedEof(8))?;
        self.offset += 8;
        Ok(f64::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_length_delimited(&mut self) -> Result<&'a [u8], WireError> {
        let length = self.read_varint_raw()? as usize;
        let slice = self
            .data
            .get(self.offset..self.offset + length)
            .ok_or(WireError::UnexpectedEof(length))?;
        self.offset += length;
        Ok(slice)
    }

    pub fn read_string(&mut self) -> Result<String, WireError> {
        let bytes = self.read_length_delimited()?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn read_packed_varints(&mut self) -> Result<Vec<u64>, WireError> {
        let bytes = self.read_length_delimited()?;
        let mut values = Vec::new();
        let mut pos = 0usize;
        while pos < bytes.len() {
            let (value, next) = decode_varint(bytes, pos)?;
            values.push(value);
            pos = next;
        }
        Ok(values)
    }

    pub fn skip(&mut self, wire_type: u32) -> Result<(), WireError> {
        match wire_type {
            WIRE_VARINT => {
                self.read_varint_raw()?;
                Ok(())
            }
            WIRE_64BIT => {
                self.offset = self.offset.checked_add(8).ok_or(WireError::UnexpectedEof(8))?;
                if self.offset > self.data.len() {
                    return Err(WireError::UnexpectedEof(8));
                }
                Ok(())
            }
            WIRE_LENGTH_DELIMITED => {
                self.read_length_delimited()?;
                Ok(())
            }
            WIRE_32BIT => {
                self.offset = self.offset.checked_add(4).ok_or(WireError::UnexpectedEof(4))?;
                if self.offset > self.data.len() {
                    return Err(WireError::UnexpectedEof(4));
                }
                Ok(())
            }
            other => Err(WireError::UnknownWireType(other)),
        }
    }
}

// ---------------------------------------------------------------------
// BytesCodec-style length-prefix framing for raw TCP (hbbs:21116,
// hbbr:21117). remote-core never needed this (browser WebSocket frames
// already delimit one message each) — this is new territory. Variable
// 1-4 byte header: the low 2 bits of the first byte select the header
// width, and the frame length is the header's value right-shifted by 2.
// ---------------------------------------------------------------------

const MAX_FRAME_LEN: u64 = 128 * 1024 * 1024;

/// Encodes `payload`'s length as a `BytesCodec`-style variable header and
/// appends `payload`, returning the full frame.
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, WireError> {
    let len = payload.len() as u64;
    if len > MAX_FRAME_LEN {
        return Err(WireError::FrameTooLarge);
    }
    // header = (len << 2) | width_selector; smallest width that fits wins.
    let mut header_frame = Vec::with_capacity(4 + payload.len());
    if len < (1 << 6) {
        header_frame.push((len << 2) as u8);
    } else if len < (1 << 14) {
        let header = (len << 2) | 1;
        header_frame.push((header & 0xff) as u8);
        header_frame.push((header >> 8) as u8);
    } else if len < (1 << 22) {
        let header = (len << 2) | 2;
        header_frame.push((header & 0xff) as u8);
        header_frame.push(((header >> 8) & 0xff) as u8);
        header_frame.push((header >> 16) as u8);
    } else {
        let header = (len << 2) | 3;
        header_frame.push((header & 0xff) as u8);
        header_frame.push(((header >> 8) & 0xff) as u8);
        header_frame.push(((header >> 16) & 0xff) as u8);
        header_frame.push((header >> 24) as u8);
    }
    header_frame.extend_from_slice(payload);
    Ok(header_frame)
}

/// Attempts to decode one frame's header+payload from the front of `buf`.
/// Returns `Ok(None)` if `buf` doesn't yet contain a full frame (the caller
/// should read more bytes and retry) — this is the shape `tokio_util`'s
/// `Decoder::decode` expects.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(usize, Vec<u8>)>, WireError> {
    let Some(&first) = buf.first() else {
        return Ok(None);
    };
    let width = match first & 0x3 {
        0 => 1,
        1 => 2,
        2 => 3,
        _ => 4,
    };
    if buf.len() < width {
        return Ok(None);
    }
    let mut header: u64 = 0;
    for (i, &byte) in buf.iter().take(width).enumerate() {
        header |= u64::from(byte) << (8 * i);
    }
    let len = header >> 2;
    if len > MAX_FRAME_LEN {
        return Err(WireError::FrameTooLarge);
    }
    let len = len as usize;
    let total = width + len;
    if buf.len() < total {
        return Ok(None);
    }
    Ok(Some((total, buf[width..total].to_vec())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trips_small_and_large_values() {
        for value in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut out = Vec::new();
            encode_varint(value, &mut out);
            let (decoded, next) = decode_varint(&out, 0).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(next, out.len());
        }
    }

    #[test]
    fn varint_matches_known_protobuf_example() {
        // Google's own published example: 300 encodes as [0xAC, 0x02].
        let mut out = Vec::new();
        encode_varint(300, &mut out);
        assert_eq!(out, vec![0xAC, 0x02]);
    }

    #[test]
    fn decode_varint_truncated_errors() {
        assert_eq!(decode_varint(&[0x80], 0), Err(WireError::TruncatedVarint));
    }

    #[test]
    fn zigzag_round_trips_including_negatives() {
        for value in [0i32, 1, -1, 2, -2, i32::MAX, i32::MIN] {
            assert_eq!(zigzag_decode32(zigzag_encode32(value)), value);
        }
    }

    #[test]
    fn zigzag_matches_known_protobuf_mapping() {
        // proto3 spec's own worked examples: 0->0, -1->1, 1->2, -2->3, 2->4.
        assert_eq!(zigzag_encode32(0), 0);
        assert_eq!(zigzag_encode32(-1), 1);
        assert_eq!(zigzag_encode32(1), 2);
        assert_eq!(zigzag_encode32(-2), 3);
        assert_eq!(zigzag_encode32(2), 4);
    }

    #[test]
    fn writer_omits_default_values() {
        let mut w = ProtoWriter::new();
        w.int32(1, 0).bool(2, false).string(3, "").bytes(4, &[]);
        assert!(w.finish().is_empty());
    }

    #[test]
    fn writer_reader_round_trip_all_scalar_types() {
        let mut w = ProtoWriter::new();
        w.int32(1, -42)
            .uint32(2, 42)
            .sint32(3, -42)
            .bool(4, true)
            .int64(5, -9_000_000_000)
            .uint64(6, 9_000_000_000)
            .double(7, 3.5)
            .string(8, "hello")
            .bytes(9, &[1, 2, 3]);
        let encoded = w.finish();

        let mut r = ProtoReader::new(&encoded);
        assert_eq!(r.read_tag().unwrap().field_number, 1);
        assert_eq!(r.read_int32().unwrap(), -42);
        assert_eq!(r.read_tag().unwrap().field_number, 2);
        assert_eq!(r.read_uint32().unwrap(), 42);
        assert_eq!(r.read_tag().unwrap().field_number, 3);
        assert_eq!(r.read_sint32().unwrap(), -42);
        assert_eq!(r.read_tag().unwrap().field_number, 4);
        assert!(r.read_bool().unwrap());
        assert_eq!(r.read_tag().unwrap().field_number, 5);
        assert_eq!(r.read_int64().unwrap(), -9_000_000_000);
        assert_eq!(r.read_tag().unwrap().field_number, 6);
        assert_eq!(r.read_uint64().unwrap(), 9_000_000_000);
        assert_eq!(r.read_tag().unwrap().field_number, 7);
        assert!((r.read_double().unwrap() - 3.5).abs() < f64::EPSILON);
        assert_eq!(r.read_tag().unwrap().field_number, 8);
        assert_eq!(r.read_string().unwrap(), "hello");
        assert_eq!(r.read_tag().unwrap().field_number, 9);
        assert_eq!(r.read_length_delimited().unwrap(), &[1, 2, 3]);
        assert!(r.eof());
    }

    #[test]
    fn unknown_field_can_be_skipped() {
        let mut w = ProtoWriter::new();
        w.string(99, "unknown field");
        w.int32(1, 7);
        let encoded = w.finish();

        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 99);
        r.skip(tag.wire_type).unwrap();
        let tag2 = r.read_tag().unwrap();
        assert_eq!(tag2.field_number, 1);
        assert_eq!(r.read_int32().unwrap(), 7);
    }

    #[test]
    fn message_field_written_even_when_empty() {
        let mut w = ProtoWriter::new();
        w.message(1, &[]);
        assert!(!w.finish().is_empty());
    }

    #[test]
    fn packed_varint_round_trips() {
        let mut w = ProtoWriter::new();
        w.packed_varint(1, &[1, 2, 300]);
        let encoded = w.finish();
        let mut r = ProtoReader::new(&encoded);
        r.read_tag().unwrap();
        assert_eq!(r.read_packed_varints().unwrap(), vec![1, 2, 300]);
    }

    // --- frame length-prefix tests ---

    #[test]
    fn frame_round_trips_across_all_header_widths() {
        for len in [0usize, 10, 63, 64, 100, 16_383, 16_384, 100_000, 4_194_303, 4_194_304, 1_000_000] {
            let payload = vec![0xABu8; len];
            let frame = encode_frame(&payload).unwrap();
            let (consumed, decoded) = decode_frame(&frame).unwrap().unwrap();
            assert_eq!(consumed, frame.len());
            assert_eq!(decoded, payload);
        }
    }

    #[test]
    fn frame_header_width_selection_matches_documented_thresholds() {
        // < 2^6 => 1-byte header (low 2 bits = 0)
        let frame = encode_frame(&[0u8; 10]).unwrap();
        assert_eq!(frame[0] & 0x3, 0);
        // >= 2^6, < 2^14 => 2-byte header (low 2 bits = 1)
        let frame = encode_frame(&[0u8; 100]).unwrap();
        assert_eq!(frame[0] & 0x3, 1);
        // >= 2^14, < 2^22 => 3-byte header (low 2 bits = 2)
        let frame = encode_frame(&vec![0u8; 20_000]).unwrap();
        assert_eq!(frame[0] & 0x3, 2);
        // >= 2^22 => 4-byte header (low 2 bits = 3)
        let frame = encode_frame(&vec![0u8; 5_000_000]).unwrap();
        assert_eq!(frame[0] & 0x3, 3);
    }

    #[test]
    fn decode_frame_returns_none_on_incomplete_buffer() {
        let full = encode_frame(&[1, 2, 3, 4, 5]).unwrap();
        // Header present but payload truncated.
        assert_eq!(decode_frame(&full[..full.len() - 1]).unwrap(), None);
        // Not even the header present (for a multi-byte header case).
        let full_big = encode_frame(&[0u8; 100]).unwrap();
        assert_eq!(decode_frame(&full_big[..1]).unwrap(), None);
    }

    #[test]
    fn decode_frame_never_panics_on_garbage_or_truncated_input() {
        for len in 0..8 {
            let garbage = vec![0xFFu8; len];
            // Must not panic; any Result is acceptable.
            let _ = decode_frame(&garbage);
        }
    }

    #[test]
    fn decode_frame_rejects_absurd_length_claim() {
        // 4-byte header claiming a length far beyond MAX_FRAME_LEN.
        let header = (u64::from(u32::MAX) << 2) | 3;
        let bytes = header.to_le_bytes();
        let frame = vec![bytes[0], bytes[1], bytes[2], bytes[3]];
        assert_eq!(decode_frame(&frame), Err(WireError::FrameTooLarge));
    }
}
