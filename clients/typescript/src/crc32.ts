// SPDX-License-Identifier: AGPL-3.0-only
/**
 * Fast IEEE 802.3 standard CRC32 implementation using precomputed table.
 * Matches crc32fast in Rust and zlib.crc32 in Python.
 */

const CRC_TABLE = new Uint32Array(256);

// Initialize standard CRC32 polynomial table (0xEDB88320)
for (let i = 0; i < 256; i++) {
  let c = i;
  for (let j = 0; j < 8; j++) {
    c = (c & 1) ? (0xedb88320 ^ (c >>> 1)) : (c >>> 1);
  }
  CRC_TABLE[i] = c >>> 0;
}

export function crc32(buffer: Uint8Array): number {
  let crc = 0xffffffff;
  for (let i = 0; i < buffer.length; i++) {
    crc = (crc >>> 8) ^ CRC_TABLE[(crc ^ buffer[i]) & 0xff];
  }
  return (crc ^ 0xffffffff) >>> 0;
}
