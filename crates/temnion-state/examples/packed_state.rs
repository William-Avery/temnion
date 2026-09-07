// SPDX-License-Identifier: AGPL-3.0-only
use temnion_core::ShardId;
use temnion_state::StateSlab;

#[derive(Debug)]
struct State {
    x: i32,
    health: u16,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut slab = StateSlab::new(ShardId(0), 1024)?;
    let entity = slab.insert(State { x: 12, health: 100 })?;
    slab.get_mut(entity)?.health = 90;
    let state = slab.get(entity)?;
    println!("entity={entity:?} x={} health={}", state.x, state.health);
    slab.remove(entity)?;
    assert!(slab.get(entity).is_err());
    Ok(())
}
