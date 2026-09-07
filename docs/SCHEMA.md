# Temnion scalar schema foundation

`temnion-schema` defines Temnion's M2 scalar typed payload layer. It covers:

- stable `SchemaId` / `FieldId` types and immutable schema definitions;
- fixed scalar field metadata (`bool`, `i64`, `u64`, exact `f64` bits, UTF-8, bytes);
- per-field nullability and optional unit strings;
- sparse `Upsert` / `Delete` mutations over owned typed values; and
- strict versioned little-endian binary encodings for schemas and mutations.

Run the scalar schema/encoding/apply example:

```text
cargo run -p temnion-schema --example typed_mutation
```

## Current scope

This layer is intentionally narrow. It does **not** yet implement:

- arbitrary tensor, N-D, or vector schemas;
- schema migration tooling or compatibility transforms;
- secondary field indexes; or
- model-specific semantics.

Schema metadata is immutable after construction. Field IDs are sorted
canonically by ascending `FieldId`, names must be unique ASCII identifiers, and
units are bounded validated UTF-8 strings. Current metadata limits are:

- maximum fields per schema: 4,096;
- maximum field name length: 64 UTF-8 bytes;
- maximum field unit length: 32 UTF-8 bytes.

## Exact scalar semantics

Floating-point values use `Value::F64Bits(u64)`. Callers store
`f64::to_bits(value)` and recover the value with `f64::from_bits(bits)`. This
preserves `-0.0`, infinities, and NaN payloads exactly instead of relying on
lossy text or platform layout.

Sparse upserts only touch the listed fields. Unspecified fields are not
implicitly overwritten. Applying an upsert validates the entire resulting state
before committing it, so non-nullable fields must be present in the final state
and failed mutations leave the original state unchanged.

## Binary encoding

Both schema and mutation encodings are:

- explicitly versioned;
- little-endian;
- canonical in ascending `FieldId` order;
- length-bounded before allocation; and
- strict about truncation, trailing bytes, invalid tags, invalid booleans, and
  invalid UTF-8.

Schema IDs and field IDs are stable durable identities. They must not be
repurposed without an explicit migration path and new compatibility fixtures.
