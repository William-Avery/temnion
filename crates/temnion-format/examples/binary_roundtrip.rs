// SPDX-License-Identifier: AGPL-3.0-only
use temnion_core::{
    ClockId, DatabaseId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId,
    Timestamp,
};
use temnion_format::{Limits, StoredEvent, WalHeader, decode_segment, encode_segment};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let header = WalHeader {
        database: DatabaseId([1; 16]),
        source: SourceId(1),
        epoch: SourceEpoch(1),
    };
    let records = vec![StoredEvent {
        id: EventId {
            source: header.source,
            epoch: header.epoch,
            sequence: 0,
        },
        entity: EntityId {
            shard: ShardId(0),
            slot: 1,
            generation: 0,
        },
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), 10),
            observed: None,
            known: Timestamp::new(ClockId(2), 20),
        },
        schema: SchemaId(1),
        payload: 42u64.to_le_bytes().to_vec(),
        causes: Vec::new(),
    }];
    let bytes = encode_segment(header, &records, &Limits::default())?;
    let decoded = decode_segment(&bytes, &Limits::default())?;
    assert_eq!(decoded.header, header);
    assert_eq!(decoded.records, records);
    println!(
        "TSF bytes={} exact_records={}",
        bytes.len(),
        decoded.records.len()
    );
    Ok(())
}
