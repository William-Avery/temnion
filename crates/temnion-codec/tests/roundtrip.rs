// SPDX-License-Identifier: AGPL-3.0-only
use temnion_codec::{
    BitPackCodec, Codec, CodecError, CodecId, CodecScorer, DeltaForCodec, RawCodec, RleCodec,
    XorCodec,
};

#[test]
fn raw_codec_preserves_every_input_and_enforces_checksum() {
    let codec = RawCodec;
    let inputs: [&[u8]; 4] = [b"", b"hello world", &[0u8; 100], &[255u8; 128]];
    for input in inputs {
        let encoded = codec.encode(input).unwrap();
        let decoded = codec.decode(&encoded, input.len()).unwrap();
        assert_eq!(decoded, input);
    }

    let mut corrupted = codec.encode(b"valid payload").unwrap();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xFF;
    assert_eq!(
        codec.decode(&corrupted, 13),
        Err(CodecError::ChecksumMismatch)
    );
}

#[test]
fn rle_codec_compresses_runs_and_preserves_literals() {
    let codec = RleCodec;
    let mut data = Vec::new();
    data.extend_from_slice(&[42u8; 50]); // Run of 50
    data.extend_from_slice(b"mixed literals");
    data.extend_from_slice(&[0u8; 80]); // Run of 80

    let encoded = codec.encode(&data).unwrap();
    assert!(
        encoded.len() < data.len(),
        "RLE should compress long runs: encoded={} original={}",
        encoded.len(),
        data.len()
    );

    let decoded = codec.decode(&encoded, data.len()).unwrap();
    assert_eq!(decoded, data);

    // Roundtrip on arbitrary bytes
    for len in [0, 1, 2, 3, 4, 15, 64, 255] {
        let sample: Vec<u8> = (0..len).map(|i| (i * 7 % 256) as u8).collect();
        let enc = codec.encode(&sample).unwrap();
        let dec = codec.decode(&enc, sample.len()).unwrap();
        assert_eq!(dec, sample);
    }
}

#[test]
fn bitpack_codec_compresses_small_integers() {
    let codec = BitPackCodec;
    // 256 u32 integers all <= 15 (fits in 4 bits each)
    let mut data = Vec::new();
    for i in 0..256u32 {
        data.extend_from_slice(&(i % 16).to_le_bytes());
    }

    let encoded = codec.encode(&data).unwrap();
    assert!(
        encoded.len() < data.len() / 2,
        "BitPack should achieve ~4x compression on 4-bit numbers"
    );

    let decoded = codec.decode(&encoded, data.len()).unwrap();
    assert_eq!(decoded, data);

    // Unaligned remainder test
    let unaligned = vec![1, 2, 3, 4, 5];
    let enc = codec.encode(&unaligned).unwrap();
    let dec = codec.decode(&enc, unaligned.len()).unwrap();
    assert_eq!(dec, unaligned);
}

#[test]
fn delta_for_codec_compresses_clustered_and_monotonic_sequences() {
    let codec = DeltaForCodec;
    // 1000 timestamps starting from 1,000,000 with small increments
    let base = 1_000_000_000u64;
    let mut data = Vec::new();
    for i in 0..1000u64 {
        let ts = base + i * 10;
        data.extend_from_slice(&ts.to_le_bytes());
    }

    let encoded = codec.encode(&data).unwrap();
    assert!(
        encoded.len() < data.len() / 3,
        "Delta/FOR should achieve high compression on clustered timestamps: encoded={} original={}",
        encoded.len(),
        data.len()
    );

    let decoded = codec.decode(&encoded, data.len()).unwrap();
    assert_eq!(decoded, data);

    // Empty and small inputs
    for sample in [&b""[..], &b"1234567"[..], &b"12345678"[..]] {
        let enc = codec.encode(sample).unwrap();
        let dec = codec.decode(&enc, sample.len()).unwrap();
        assert_eq!(dec, *sample);
    }
}

#[test]
fn xor_codec_compresses_smooth_and_repeated_float_bits() {
    let codec = XorCodec;
    // Simulate time-series float values with minor changes
    let mut data = Vec::new();
    let mut val = 100.0f64;
    for _ in 0..500 {
        data.extend_from_slice(&val.to_bits().to_le_bytes());
        val += 0.05;
    }

    let encoded = codec.encode(&data).unwrap();
    assert!(
        encoded.len() < data.len(),
        "XOR should compress slowly varying floats: encoded={} original={}",
        encoded.len(),
        data.len()
    );

    let decoded = codec.decode(&encoded, data.len()).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn candidate_scorer_selects_winning_codec_and_falls_back_to_raw() {
    let scorer = CodecScorer {
        alpha_decode: 0.0,
        beta_encode: 0.0,
    };

    // Repeated data should pick RLE
    let run_data = vec![0xAA; 500];
    let eval_rle = scorer.select_best(&run_data).unwrap();
    assert_eq!(eval_rle.codec, CodecId::Rle);
    assert!(eval_rle.verified_lossless);

    // Clustered timestamps should pick DeltaFor or BitPack
    let base = 500_000u64;
    let mut ts_data = Vec::new();
    for i in 0..200u64 {
        ts_data.extend_from_slice(&(base + i).to_le_bytes());
    }
    let eval_ts = scorer.select_best(&ts_data).unwrap();
    assert!(eval_ts.compressed_bytes < eval_ts.original_bytes);
    assert!(eval_ts.verified_lossless);

    // High-entropy random data where compression doesn't help should fall back to Raw
    let random_data: Vec<u8> = (0..100).map(|i| ((i * 109 + 37) % 256) as u8).collect();
    let eval_raw = scorer.select_best(&random_data).unwrap();
    assert_eq!(eval_raw.codec, CodecId::Raw);
    assert!(eval_raw.verified_lossless);
}
