// SPDX-License-Identifier: AGPL-3.0-only
use std::collections::BTreeMap;

use temnion_core::{FieldId, SchemaId};
use temnion_schema::{
    Field, FieldType, FieldUpdate, Mutation, Schema, Value, decode_mutation, decode_schema,
    encode_mutation, encode_schema,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = Schema::new(
        SchemaId(1),
        1,
        vec![Field {
            id: FieldId(0),
            name: "health".into(),
            kind: FieldType::U64,
            nullable: false,
            unit: Some("points".into()),
        }],
    )?;
    let schema_bytes = encode_schema(&schema, 1024)?;
    let schema = decode_schema(&schema_bytes, 1024)?;
    let mutation = Mutation::Upsert(vec![FieldUpdate {
        field: FieldId(0),
        value: Value::U64(100),
    }]);
    let payload = encode_mutation(&mutation, 1024)?;
    let decoded = decode_mutation(&payload, 1024)?;
    let mut state = BTreeMap::new();
    schema.apply(&mut state, &decoded)?;
    assert_eq!(state.get(&FieldId(0)), Some(&Value::U64(100)));
    println!(
        "schema_bytes={} mutation_bytes={} state={state:?}",
        schema_bytes.len(),
        payload.len()
    );
    Ok(())
}
