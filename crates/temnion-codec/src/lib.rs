// SPDX-License-Identifier: AGPL-3.0-only
//! Lossless compression codecs and candidate scoring framework.
//!
//! Following Temnion Architecture §12:
//! - All codecs are strictly lossless and validated against the raw reference.
//! - Candidate codecs (BitPack, Delta/FOR, XOR, RLE) are evaluated against the
//!   incumbent raw baseline.
//! - The simpler incumbent remains if a candidate does not measurably win.

use std::error::Error;
use std::fmt;

const CODEC_MAGIC: [u8; 4] = *b"TNCX";
const CODEC_VERSION: u8 = 1;
pub const CODEC_HEADER_LEN: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum CodecId {
    Raw = 0,
    Rle = 1,
    BitPack = 2,
    DeltaFor = 3,
    Xor = 4,
}

impl CodecId {
    pub fn from_u8(value: u8) -> Result<Self, CodecError> {
        match value {
            0 => Ok(Self::Raw),
            1 => Ok(Self::Rle),
            2 => Ok(Self::BitPack),
            3 => Ok(Self::DeltaFor),
            4 => Ok(Self::Xor),
            other => Err(CodecError::UnknownCodec(other)),
        }
    }
}

impl fmt::Display for CodecId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Raw => write!(f, "raw"),
            Self::Rle => write!(f, "rle"),
            Self::BitPack => write!(f, "bitpack"),
            Self::DeltaFor => write!(f, "delta-for"),
            Self::Xor => write!(f, "xor"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    Incomplete { needed: usize, available: usize },
    InvalidMagic,
    UnsupportedVersion(u8),
    ChecksumMismatch,
    LengthMismatch { expected: usize, actual: usize },
    UnknownCodec(u8),
    InvalidData(&'static str),
    AllocationFailed,
    OutputTooLarge { len: usize, max: usize },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete { needed, available } => {
                write!(
                    f,
                    "incomplete codec data: needed {needed}, available {available}"
                )
            }
            Self::InvalidMagic => write!(f, "invalid codec magic bytes; expected TNCX"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported codec version {v}"),
            Self::ChecksumMismatch => write!(f, "codec checksum verification failed"),
            Self::LengthMismatch { expected, actual } => {
                write!(
                    f,
                    "decompressed length mismatch: expected {expected}, actual {actual}"
                )
            }
            Self::UnknownCodec(id) => write!(f, "unknown codec id {id}"),
            Self::InvalidData(msg) => write!(f, "invalid compressed data: {msg}"),
            Self::AllocationFailed => write!(f, "failed to allocate memory for decompression"),
            Self::OutputTooLarge { len, max } => {
                write!(f, "decompressed output {len} exceeds limit {max}")
            }
        }
    }
}

impl Error for CodecError {}

/// Lossless codec abstraction for column and stream compression.
pub trait Codec: Send + Sync {
    fn id(&self) -> CodecId;
    fn name(&self) -> &'static str;
    fn encode(&self, input: &[u8]) -> Result<Vec<u8>, CodecError>;
    fn decode(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodecError>;
}

/// Helper to encode a standardized framing header around compressed bytes.
fn frame_compressed(id: CodecId, original_len: usize, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    if original_len > u32::MAX as usize {
        return Err(CodecError::OutputTooLarge {
            len: original_len,
            max: u32::MAX as usize,
        });
    }
    let mut header = [0u8; CODEC_HEADER_LEN];
    header[0..4].copy_from_slice(&CODEC_MAGIC);
    header[4] = id as u8;
    header[5] = CODEC_VERSION;
    header[6..8].copy_from_slice(&0u16.to_le_bytes()); // flags = 0
    header[8..12].copy_from_slice(&(original_len as u32).to_le_bytes());
    let body_crc = crc32fast::hash(body);
    header[12..16].copy_from_slice(&body_crc.to_le_bytes());
    let header_crc = crc32fast::hash(&header[0..16]);
    header[16..20].copy_from_slice(&header_crc.to_le_bytes());

    let mut out = Vec::new();
    out.try_reserve_exact(CODEC_HEADER_LEN + body.len())
        .map_err(|_| CodecError::AllocationFailed)?;
    out.extend_from_slice(&header);
    out.extend_from_slice(body);
    Ok(out)
}

/// Helper to validate the standardized framing header and extract the body.
fn verify_frame(
    expected_id: CodecId,
    compressed: &[u8],
    expected_len: usize,
) -> Result<&[u8], CodecError> {
    if compressed.len() < CODEC_HEADER_LEN {
        return Err(CodecError::Incomplete {
            needed: CODEC_HEADER_LEN,
            available: compressed.len(),
        });
    }
    if compressed[0..4] != CODEC_MAGIC {
        return Err(CodecError::InvalidMagic);
    }
    let codec_id = CodecId::from_u8(compressed[4])?;
    if codec_id != expected_id {
        return Err(CodecError::InvalidData("codec id mismatch in frame header"));
    }
    let version = compressed[5];
    if version != CODEC_VERSION {
        return Err(CodecError::UnsupportedVersion(version));
    }
    let header_crc = u32::from_le_bytes(compressed[16..20].try_into().unwrap());
    if crc32fast::hash(&compressed[0..16]) != header_crc {
        return Err(CodecError::ChecksumMismatch);
    }
    let original_len = u32::from_le_bytes(compressed[8..12].try_into().unwrap()) as usize;
    if original_len != expected_len {
        return Err(CodecError::LengthMismatch {
            expected: expected_len,
            actual: original_len,
        });
    }
    let body = &compressed[CODEC_HEADER_LEN..];
    let body_crc = u32::from_le_bytes(compressed[12..16].try_into().unwrap());
    if crc32fast::hash(body) != body_crc {
        return Err(CodecError::ChecksumMismatch);
    }
    Ok(body)
}

// ---------------------------------------------------------------------------
// Raw Codec (Incumbent Baseline)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct RawCodec;

impl Codec for RawCodec {
    fn id(&self) -> CodecId {
        CodecId::Raw
    }

    fn name(&self) -> &'static str {
        "raw"
    }

    fn encode(&self, input: &[u8]) -> Result<Vec<u8>, CodecError> {
        frame_compressed(CodecId::Raw, input.len(), input)
    }

    fn decode(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodecError> {
        let body = verify_frame(CodecId::Raw, compressed, expected_len)?;
        let mut out = Vec::new();
        out.try_reserve_exact(body.len())
            .map_err(|_| CodecError::AllocationFailed)?;
        out.extend_from_slice(body);
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Run-Length Encoding (RLE) Codec
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct RleCodec;

impl Codec for RleCodec {
    fn id(&self) -> CodecId {
        CodecId::Rle
    }

    fn name(&self) -> &'static str {
        "rle"
    }

    fn encode(&self, input: &[u8]) -> Result<Vec<u8>, CodecError> {
        let mut body = Vec::new();
        let mut i = 0;
        while i < input.len() {
            // Check for run of identical bytes
            let b = input[i];
            let mut run_len = 1;
            while i + run_len < input.len() && input[i + run_len] == b && run_len < 130 {
                run_len += 1;
            }

            if run_len >= 3 {
                // Encode as run: tag 0x80 | (run_len - 3), followed by byte b
                let tag = 0x80 | ((run_len - 3) as u8);
                body.push(tag);
                body.push(b);
                i += run_len;
            } else {
                // Collect literals
                let lit_start = i;
                let mut lit_len = 0;
                while i < input.len() && lit_len < 128 {
                    // Stop if a run of >= 3 begins
                    if i + 2 < input.len() && input[i] == input[i + 1] && input[i] == input[i + 2] {
                        break;
                    }
                    lit_len += 1;
                    i += 1;
                }
                let tag = (lit_len - 1) as u8;
                body.push(tag);
                body.extend_from_slice(&input[lit_start..lit_start + lit_len]);
            }
        }
        frame_compressed(CodecId::Rle, input.len(), &body)
    }

    fn decode(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodecError> {
        let body = verify_frame(CodecId::Rle, compressed, expected_len)?;
        let mut out = Vec::new();
        out.try_reserve_exact(expected_len)
            .map_err(|_| CodecError::AllocationFailed)?;

        let mut i = 0;
        while i < body.len() {
            let tag = body[i];
            i += 1;
            if (tag & 0x80) != 0 {
                let run_len = ((tag & 0x7F) as usize) + 3;
                if i >= body.len() {
                    return Err(CodecError::InvalidData("truncated RLE run payload"));
                }
                let b = body[i];
                i += 1;
                if out.len() + run_len > expected_len {
                    return Err(CodecError::InvalidData("RLE run exceeds expected length"));
                }
                out.resize(out.len() + run_len, b);
            } else {
                let lit_len = (tag as usize) + 1;
                if i + lit_len > body.len() {
                    return Err(CodecError::InvalidData("truncated RLE literal payload"));
                }
                if out.len() + lit_len > expected_len {
                    return Err(CodecError::InvalidData(
                        "RLE literals exceed expected length",
                    ));
                }
                out.extend_from_slice(&body[i..i + lit_len]);
                i += lit_len;
            }
        }

        if out.len() != expected_len {
            return Err(CodecError::LengthMismatch {
                expected: expected_len,
                actual: out.len(),
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Bit-Packing Codec (Uniform Width u32 Words)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct BitPackCodec;

impl Codec for BitPackCodec {
    fn id(&self) -> CodecId {
        CodecId::BitPack
    }

    fn name(&self) -> &'static str {
        "bitpack"
    }

    fn encode(&self, input: &[u8]) -> Result<Vec<u8>, CodecError> {
        let word_count = input.len() / 4;
        let remainder_len = input.len() % 4;

        let mut max_val = 0u32;
        let mut words = Vec::with_capacity(word_count);
        for chunk in input[0..word_count * 4].chunks_exact(4) {
            let word = u32::from_le_bytes(chunk.try_into().unwrap());
            if word > max_val {
                max_val = word;
            }
            words.push(word);
        }

        let bits = if max_val == 0 {
            0u8
        } else {
            32 - max_val.leading_zeros() as u8
        };

        let mut body = Vec::new();
        body.push(bits);
        body.extend_from_slice(&(word_count as u32).to_le_bytes());

        if bits > 0 {
            let mut bit_buf = 0u64;
            let mut bit_count = 0u8;
            for &word in &words {
                bit_buf |= (word as u64) << bit_count;
                bit_count += bits;
                while bit_count >= 8 {
                    body.push(bit_buf as u8);
                    bit_buf >>= 8;
                    bit_count -= 8;
                }
            }
            if bit_count > 0 {
                body.push(bit_buf as u8);
            }
        }

        // Remainder unaligned bytes appended as-is
        if remainder_len > 0 {
            body.extend_from_slice(&input[word_count * 4..]);
        }

        frame_compressed(CodecId::BitPack, input.len(), &body)
    }

    fn decode(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodecError> {
        let body = verify_frame(CodecId::BitPack, compressed, expected_len)?;
        if body.len() < 5 {
            return Err(CodecError::Incomplete {
                needed: 5,
                available: body.len(),
            });
        }
        let bits = body[0];
        if bits > 32 {
            return Err(CodecError::InvalidData("bit-width exceeds 32"));
        }
        let word_count = u32::from_le_bytes(body[1..5].try_into().unwrap()) as usize;
        let remainder_len = expected_len.saturating_sub(word_count * 4);
        if word_count * 4 + remainder_len != expected_len {
            return Err(CodecError::InvalidData(
                "inconsistent word count and length",
            ));
        }

        let packed_bytes_len = if bits == 0 {
            0
        } else {
            (word_count * bits as usize).div_ceil(8)
        };

        if body.len() < 5 + packed_bytes_len + remainder_len {
            return Err(CodecError::Incomplete {
                needed: 5 + packed_bytes_len + remainder_len,
                available: body.len(),
            });
        }

        let mut out = Vec::new();
        out.try_reserve_exact(expected_len)
            .map_err(|_| CodecError::AllocationFailed)?;

        if bits == 0 {
            for _ in 0..word_count {
                out.extend_from_slice(&0u32.to_le_bytes());
            }
        } else {
            let packed_slice = &body[5..5 + packed_bytes_len];
            let mut byte_idx = 0;
            let mut bit_buf = 0u64;
            let mut bit_count = 0u8;
            let mask = (1u64 << bits) - 1;

            for _ in 0..word_count {
                while bit_count < bits && byte_idx < packed_slice.len() {
                    bit_buf |= (packed_slice[byte_idx] as u64) << bit_count;
                    byte_idx += 1;
                    bit_count += 8;
                }
                let word = (bit_buf & mask) as u32;
                bit_buf >>= bits;
                bit_count = bit_count.saturating_sub(bits);
                out.extend_from_slice(&word.to_le_bytes());
            }
        }

        if remainder_len > 0 {
            let rem_slice = &body[5 + packed_bytes_len..5 + packed_bytes_len + remainder_len];
            out.extend_from_slice(rem_slice);
        }

        if out.len() != expected_len {
            return Err(CodecError::LengthMismatch {
                expected: expected_len,
                actual: out.len(),
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Delta + Frame-of-Reference (FOR) Codec
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct DeltaForCodec;

impl Codec for DeltaForCodec {
    fn id(&self) -> CodecId {
        CodecId::DeltaFor
    }

    fn name(&self) -> &'static str {
        "delta-for"
    }

    fn encode(&self, input: &[u8]) -> Result<Vec<u8>, CodecError> {
        let u64_count = input.len() / 8;
        let remainder_len = input.len() % 8;

        if u64_count == 0 {
            // Delegate trivial inputs directly (8 bytes base + 1 byte bits + 4 bytes count = 13 bytes)
            let mut body = vec![0u8; 13];
            body.extend_from_slice(input);
            return frame_compressed(CodecId::DeltaFor, input.len(), &body);
        }

        let mut values = Vec::with_capacity(u64_count);
        for chunk in input[0..u64_count * 8].chunks_exact(8) {
            values.push(u64::from_le_bytes(chunk.try_into().unwrap()));
        }

        // Compute Frame-of-Reference base (minimum value)
        let min_val = *values.iter().min().unwrap();
        let mut max_offset = 0u64;
        let mut offsets = Vec::with_capacity(u64_count);
        for &v in &values {
            let offset = v - min_val;
            if offset > max_offset {
                max_offset = offset;
            }
            offsets.push(offset);
        }

        let bits = if max_offset == 0 {
            0u8
        } else {
            64 - max_offset.leading_zeros() as u8
        };

        let mut body = Vec::new();
        body.extend_from_slice(&min_val.to_le_bytes()); // 8 bytes base
        body.push(bits); // 1 byte bit-width
        body.extend_from_slice(&(u64_count as u32).to_le_bytes()); // 4 bytes count

        if bits > 0 {
            let mut bit_buf = 0u128;
            let mut bit_count = 0u8;
            for &offset in &offsets {
                bit_buf |= (offset as u128) << bit_count;
                bit_count += bits;
                while bit_count >= 8 {
                    body.push(bit_buf as u8);
                    bit_buf >>= 8;
                    bit_count -= 8;
                }
            }
            if bit_count > 0 {
                body.push(bit_buf as u8);
            }
        }

        if remainder_len > 0 {
            body.extend_from_slice(&input[u64_count * 8..]);
        }

        frame_compressed(CodecId::DeltaFor, input.len(), &body)
    }

    fn decode(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodecError> {
        let body = verify_frame(CodecId::DeltaFor, compressed, expected_len)?;
        if body.len() < 13 {
            return Err(CodecError::Incomplete {
                needed: 13,
                available: body.len(),
            });
        }
        let min_val = u64::from_le_bytes(body[0..8].try_into().unwrap());
        let bits = body[8];
        if bits > 64 {
            return Err(CodecError::InvalidData("FOR bit-width exceeds 64"));
        }
        let u64_count = u32::from_le_bytes(body[9..13].try_into().unwrap()) as usize;
        let remainder_len = expected_len.saturating_sub(u64_count * 8);

        let packed_bytes_len = if bits == 0 {
            0
        } else {
            (u64_count * bits as usize).div_ceil(8)
        };

        if body.len() < 13 + packed_bytes_len + remainder_len {
            return Err(CodecError::Incomplete {
                needed: 13 + packed_bytes_len + remainder_len,
                available: body.len(),
            });
        }

        let mut out = Vec::new();
        out.try_reserve_exact(expected_len)
            .map_err(|_| CodecError::AllocationFailed)?;

        if bits == 0 {
            for _ in 0..u64_count {
                out.extend_from_slice(&min_val.to_le_bytes());
            }
        } else {
            let packed_slice = &body[13..13 + packed_bytes_len];
            let mut byte_idx = 0;
            let mut bit_buf = 0u128;
            let mut bit_count = 0u8;
            let mask = (1u128 << bits) - 1;

            for _ in 0..u64_count {
                while bit_count < bits && byte_idx < packed_slice.len() {
                    bit_buf |= (packed_slice[byte_idx] as u128) << bit_count;
                    byte_idx += 1;
                    bit_count += 8;
                }
                let offset = (bit_buf & mask) as u64;
                bit_buf >>= bits;
                bit_count = bit_count.saturating_sub(bits);
                let val = min_val + offset;
                out.extend_from_slice(&val.to_le_bytes());
            }
        }

        if remainder_len > 0 {
            let rem_slice = &body[13 + packed_bytes_len..13 + packed_bytes_len + remainder_len];
            out.extend_from_slice(rem_slice);
        }

        if out.len() != expected_len {
            return Err(CodecError::LengthMismatch {
                expected: expected_len,
                actual: out.len(),
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// XOR / Gorilla-Style Difference Codec
// ---------------------------------------------------------------------------

struct BitWriter<'a> {
    buf: &'a mut Vec<u8>,
    bit_buf: u64,
    bit_count: u8,
}

impl<'a> BitWriter<'a> {
    fn new(buf: &'a mut Vec<u8>) -> Self {
        Self {
            buf,
            bit_buf: 0,
            bit_count: 0,
        }
    }

    fn write_bit(&mut self, bit: u8) {
        self.bit_buf |= ((bit as u64) & 1) << self.bit_count;
        self.bit_count += 1;
        if self.bit_count == 8 {
            self.buf.push(self.bit_buf as u8);
            self.bit_buf = 0;
            self.bit_count = 0;
        }
    }

    fn write_bits(&mut self, val: u64, count: u8) {
        for i in 0..count {
            self.write_bit(((val >> i) & 1) as u8);
        }
    }

    fn flush(&mut self) {
        if self.bit_count > 0 {
            self.buf.push(self.bit_buf as u8);
            self.bit_buf = 0;
            self.bit_count = 0;
        }
    }
}

struct BitReader<'a> {
    slice: &'a [u8],
    byte_idx: usize,
    bit_buf: u64,
    bit_count: u8,
}

impl<'a> BitReader<'a> {
    fn new(slice: &'a [u8]) -> Self {
        Self {
            slice,
            byte_idx: 0,
            bit_buf: 0,
            bit_count: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u8, CodecError> {
        if self.bit_count == 0 {
            if self.byte_idx >= self.slice.len() {
                return Err(CodecError::InvalidData("unexpected end of bitstream"));
            }
            self.bit_buf = self.slice[self.byte_idx] as u64;
            self.byte_idx += 1;
            self.bit_count = 8;
        }
        let bit = (self.bit_buf & 1) as u8;
        self.bit_buf >>= 1;
        self.bit_count -= 1;
        Ok(bit)
    }

    fn read_bits(&mut self, count: u8) -> Result<u64, CodecError> {
        let mut res = 0u64;
        for i in 0..count {
            let b = self.read_bit()?;
            res |= (b as u64) << i;
        }
        Ok(res)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct XorCodec;

impl Codec for XorCodec {
    fn id(&self) -> CodecId {
        CodecId::Xor
    }

    fn name(&self) -> &'static str {
        "xor"
    }

    fn encode(&self, input: &[u8]) -> Result<Vec<u8>, CodecError> {
        let u64_count = input.len() / 8;
        let remainder_len = input.len() % 8;

        let mut body = Vec::new();
        body.extend_from_slice(&(u64_count as u32).to_le_bytes());

        if u64_count > 0 {
            let mut words = Vec::with_capacity(u64_count);
            for chunk in input[0..u64_count * 8].chunks_exact(8) {
                words.push(u64::from_le_bytes(chunk.try_into().unwrap()));
            }

            // Write first value in full
            body.extend_from_slice(&words[0].to_le_bytes());

            let mut prev = words[0];
            let mut writer = BitWriter::new(&mut body);

            for &curr in &words[1..] {
                let xor = curr ^ prev;
                prev = curr;
                if xor == 0 {
                    writer.write_bit(0);
                } else {
                    writer.write_bit(1);
                    let leading = xor.leading_zeros().min(31) as u8;
                    let trailing = xor.trailing_zeros().min(31) as u8;
                    let length = 64 - leading - trailing;
                    writer.write_bits(leading as u64, 5);
                    writer.write_bits(length as u64, 6);
                    writer.write_bits(xor >> trailing, length);
                }
            }
            writer.flush();
        }

        if remainder_len > 0 {
            body.extend_from_slice(&input[u64_count * 8..]);
        }

        frame_compressed(CodecId::Xor, input.len(), &body)
    }

    fn decode(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CodecError> {
        let body = verify_frame(CodecId::Xor, compressed, expected_len)?;
        if body.len() < 4 {
            return Err(CodecError::Incomplete {
                needed: 4,
                available: body.len(),
            });
        }
        let u64_count = u32::from_le_bytes(body[0..4].try_into().unwrap()) as usize;
        let remainder_len = expected_len.saturating_sub(u64_count * 8);

        let mut out = Vec::new();
        out.try_reserve_exact(expected_len)
            .map_err(|_| CodecError::AllocationFailed)?;

        if u64_count > 0 {
            if body.len() < 12 {
                return Err(CodecError::Incomplete {
                    needed: 12,
                    available: body.len(),
                });
            }
            let first = u64::from_le_bytes(body[4..12].try_into().unwrap());
            out.extend_from_slice(&first.to_le_bytes());

            let mut prev = first;
            let mut reader = BitReader::new(&body[12..]);

            for _ in 1..u64_count {
                let bit = reader.read_bit()?;
                let curr = if bit == 0 {
                    prev
                } else {
                    let leading = reader.read_bits(5)? as u8;
                    let length = reader.read_bits(6)? as u8;
                    let meaningful = reader.read_bits(length)?;
                    let trailing = 64 - leading - length;
                    let xor = meaningful << trailing;
                    prev ^ xor
                };
                out.extend_from_slice(&curr.to_le_bytes());
                prev = curr;
            }
        }

        if remainder_len > 0 {
            let start = body.len().saturating_sub(remainder_len);
            out.extend_from_slice(&body[start..]);
        }

        if out.len() != expected_len {
            return Err(CodecError::LengthMismatch {
                expected: expected_len,
                actual: out.len(),
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Candidate Evaluation & Scoring
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct CodecEvaluation {
    pub codec: CodecId,
    pub name: &'static str,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub ratio: f64,
    pub encode_ns: u128,
    pub decode_ns: u128,
    pub score: f64,
    pub verified_lossless: bool,
}

/// Evaluates lossless codecs against input data following Architecture §12:
/// selects the winner only if it strictly outperforms the raw baseline.
pub struct CodecScorer {
    pub alpha_decode: f64,
    pub beta_encode: f64,
}

impl Default for CodecScorer {
    fn default() -> Self {
        Self {
            alpha_decode: 0.001,
            beta_encode: 0.0001,
        }
    }
}

impl CodecScorer {
    pub fn evaluate_candidate(
        &self,
        codec: &dyn Codec,
        data: &[u8],
    ) -> Result<CodecEvaluation, CodecError> {
        let t0 = std::time::Instant::now();
        let compressed = codec.encode(data)?;
        let encode_ns = t0.elapsed().as_nanos();

        let t1 = std::time::Instant::now();
        let decoded = codec.decode(&compressed, data.len())?;
        let decode_ns = t1.elapsed().as_nanos();

        let verified_lossless = decoded == data;
        if !verified_lossless {
            return Err(CodecError::InvalidData(
                "roundtrip data corruption detected",
            ));
        }

        let ratio = compressed.len() as f64 / data.len().max(1) as f64;
        let score = compressed.len() as f64
            + self.alpha_decode * (decode_ns as f64)
            + self.beta_encode * (encode_ns as f64);

        Ok(CodecEvaluation {
            codec: codec.id(),
            name: codec.name(),
            original_bytes: data.len(),
            compressed_bytes: compressed.len(),
            ratio,
            encode_ns,
            decode_ns,
            score,
            verified_lossless,
        })
    }

    /// Evaluates all standard codecs and returns the best candidate.
    /// Falls back to RawCodec if no candidate strictly improves size or score.
    pub fn select_best(&self, data: &[u8]) -> Result<CodecEvaluation, CodecError> {
        let codecs: [&dyn Codec; 5] = [
            &RawCodec,
            &RleCodec,
            &BitPackCodec,
            &DeltaForCodec,
            &XorCodec,
        ];

        let mut evaluations = Vec::new();
        for &c in &codecs {
            if let Ok(eval) = self.evaluate_candidate(c, data) {
                evaluations.push(eval);
            }
        }

        let raw_eval = evaluations
            .iter()
            .find(|e| e.codec == CodecId::Raw)
            .cloned()
            .ok_or(CodecError::InvalidData("raw codec evaluation missing"))?;

        // Candidate must be smaller than raw and have the lowest score
        let winner = evaluations
            .into_iter()
            .filter(|e| e.compressed_bytes < raw_eval.compressed_bytes)
            .min_by(|a, b| a.score.partial_cmp(&b.score).unwrap())
            .unwrap_or(raw_eval);

        Ok(winner)
    }
}
