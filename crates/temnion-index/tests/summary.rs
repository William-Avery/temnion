// SPDX-License-Identifier: AGPL-3.0-only
use temnion_core::{
    ClockId, DatabaseId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, TimeAxis,
    TimeRange, Timestamp,
};
use temnion_events::HistoryFilter;
use temnion_index::{
    BlockSummary, EntityBloomFilter, IndexError, SegmentSummary, SkipDecision, ZoneMap,
};

fn sample_entity(slot: u32) -> EntityId {
    EntityId {
        shard: ShardId(0),
        slot,
        generation: 1,
    }
}

fn sample_times(valid: u64, known: u64) -> EventTimes {
    EventTimes {
        valid: Timestamp::new(ClockId(1), valid),
        observed: None,
        known: Timestamp::new(ClockId(2), known),
    }
}

#[test]
fn zone_map_containment_and_overlap() {
    let mut zm = ZoneMap::new(10u64);
    assert_eq!(zm.min, 10);
    assert_eq!(zm.max, 10);
    assert!(zm.contains(&10));
    assert!(!zm.contains(&9));
    assert!(!zm.contains(&11));

    zm.update(25);
    zm.update(5);
    assert_eq!(zm.min, 5);
    assert_eq!(zm.max, 25);
    assert!(zm.contains(&5));
    assert!(zm.contains(&15));
    assert!(zm.contains(&25));
    assert!(!zm.contains(&4));
    assert!(!zm.contains(&26));

    let other_overlapping = ZoneMap { min: 20, max: 30 };
    assert!(zm.overlaps(&other_overlapping));

    let other_disjoint = ZoneMap { min: 30, max: 40 };
    assert!(!zm.overlaps(&other_disjoint));
}

#[test]
fn bloom_filter_guarantees_zero_false_negatives() {
    let mut bloom = EntityBloomFilter::new();
    assert!(bloom.is_empty());

    let e1 = sample_entity(100);
    let e2 = sample_entity(200);
    let e3 = sample_entity(300);

    bloom.insert(e1);
    bloom.insert(e2);
    assert!(!bloom.is_empty());

    // Present entities MUST be reported as may_contain (zero false negatives)
    assert!(bloom.may_contain(e1));
    assert!(bloom.may_contain(e2));

    // Absent entity should be rejected with high probability
    assert!(!bloom.may_contain(e3));
}

#[test]
fn block_summary_prunes_queries_conservatively() {
    let e1 = sample_entity(10);
    let e2 = sample_entity(20);
    let t1 = sample_times(100, 500);
    let t2 = sample_times(150, 550);

    let mut block = BlockSummary::new(0, 0, 1024, 0, e1, SchemaId(1), &t1);
    block.update(1, e2, SchemaId(2), &t2);

    assert_eq!(block.record_count, 2);
    assert_eq!(block.sequence_range.min, 0);
    assert_eq!(block.sequence_range.max, 1);

    // 1. Entity query matching inside the block
    let filter_hit = HistoryFilter {
        entity: Some(e1),
        time: None,
        known_as_of: None,
    };
    assert_eq!(block.matches_filter(&filter_hit), SkipDecision::MustScan);

    // 2. Entity query for an entity never inserted -> Skip
    let filter_miss_entity = HistoryFilter {
        entity: Some(sample_entity(999)),
        time: None,
        known_as_of: None,
    };
    assert_eq!(
        block.matches_filter(&filter_miss_entity),
        SkipDecision::Skip
    );

    // 3. Known-as-of cutoff strictly before the block's earliest event -> Skip
    let filter_early_cutoff = HistoryFilter {
        entity: None,
        time: None,
        known_as_of: Some(Timestamp::new(ClockId(2), 499)), // min is 500
    };
    assert_eq!(
        block.matches_filter(&filter_early_cutoff),
        SkipDecision::Skip
    );

    // 4. Known-as-of cutoff after the block's earliest event -> MustScan
    let filter_late_cutoff = HistoryFilter {
        entity: None,
        time: None,
        known_as_of: Some(Timestamp::new(ClockId(2), 500)),
    };
    assert_eq!(
        block.matches_filter(&filter_late_cutoff),
        SkipDecision::MustScan
    );

    // 5. Valid time range disjoint with [100, 150] -> Skip
    let tr_disjoint = TimeRange::new(ClockId(1), 0, 100).unwrap();
    let filter_disjoint_time = HistoryFilter {
        entity: None,
        time: Some((TimeAxis::Valid, tr_disjoint)),
        known_as_of: None,
    };
    assert_eq!(
        block.matches_filter(&filter_disjoint_time),
        SkipDecision::Skip
    );

    // 6. Valid time range overlapping [100, 150] -> MustScan
    let tr_overlapping = TimeRange::new(ClockId(1), 120, 200).unwrap();
    let filter_overlap_time = HistoryFilter {
        entity: None,
        time: Some((TimeAxis::Valid, tr_overlapping)),
        known_as_of: None,
    };
    assert_eq!(
        block.matches_filter(&filter_overlap_time),
        SkipDecision::MustScan
    );
}

#[test]
fn segment_summary_encode_decode_roundtrip_and_tamper_detection() {
    let db = DatabaseId([7u8; 16]);
    let src = SourceId(1);
    let epoch = SourceEpoch(1);

    let b0 = BlockSummary::new(
        0,
        0,
        512,
        0,
        sample_entity(1),
        SchemaId(1),
        &sample_times(10, 100),
    );
    let b1 = BlockSummary::new(
        1,
        512,
        512,
        1,
        sample_entity(2),
        SchemaId(2),
        &sample_times(20, 200),
    );

    let seg = SegmentSummary::new(db, src, epoch, vec![b0, b1]).unwrap();
    assert_eq!(seg.total_records, 2);
    assert_eq!(seg.blocks.len(), 2);
    assert_eq!(seg.sequence_range.min, 0);
    assert_eq!(seg.sequence_range.max, 1);

    let encoded = seg.encode();
    assert!(encoded.len() > 100);

    let decoded = SegmentSummary::decode(&encoded).unwrap();
    assert_eq!(decoded.database, db);
    assert_eq!(decoded.source, src);
    assert_eq!(decoded.epoch, epoch);
    assert_eq!(decoded.total_records, 2);
    assert_eq!(decoded.blocks.len(), 2);
    assert_eq!(decoded.blocks[0].batch_index, 0);
    assert_eq!(decoded.blocks[1].batch_index, 1);

    // Tampering with payload fails checksum verification
    let mut corrupted = encoded.clone();
    corrupted[15] ^= 0xff;
    assert_eq!(
        SegmentSummary::decode(&corrupted).unwrap_err(),
        IndexError::ChecksumMismatch
    );
}

#[test]
fn multi_block_pruning_achieves_high_skip_ratio() {
    let mut blocks = Vec::new();
    for i in 0..100 {
        let entity = sample_entity(i * 10);
        let times = sample_times(i as u64 * 100, i as u64 * 100);
        blocks.push(BlockSummary::new(
            i,
            i as u64 * 256,
            256,
            i as u64,
            entity,
            SchemaId(1),
            &times,
        ));
    }

    let filter_point = HistoryFilter {
        entity: Some(sample_entity(500)), // Matches block 50
        time: None,
        known_as_of: None,
    };

    let mut scanned = 0;
    let mut skipped = 0;
    for block in &blocks {
        match block.matches_filter(&filter_point) {
            SkipDecision::MustScan => scanned += 1,
            SkipDecision::Skip => skipped += 1,
        }
    }

    assert_eq!(scanned, 1);
    assert_eq!(skipped, 99);
}
