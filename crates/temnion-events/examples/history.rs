// SPDX-License-Identifier: AGPL-3.0-only
use temnion_core::{ClockId, EntityId, EventTimes, ShardId, SourceEpoch, SourceId, Timestamp};
use temnion_events::{EventInput, EventLog, HistoryFilter, QueryBudget};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entity = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 0,
    };
    let mut log = EventLog::new(SourceId(1), SourceEpoch(1), 16)?;
    let receipt = log.append(EventInput {
        entity,
        times: EventTimes {
            valid: Timestamp::new(ClockId(1), 7),
            observed: None,
            known: Timestamp::new(ClockId(2), 10),
        },
        change: 42u16,
    })?;
    println!("Volatile event admitted: {:?}", receipt.first);
    let page = log.history(
        HistoryFilter {
            entity: Some(entity),
            ..HistoryFilter::default()
        },
        QueryBudget {
            max_results: 8,
            max_scanned: 16,
        },
        None,
    )?;
    for event in page.events {
        println!("sequence={} value={}", event.id.sequence, event.change);
    }
    Ok(())
}
