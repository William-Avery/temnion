// SPDX-License-Identifier: AGPL-3.0-only
//! Typed scalar schemas and canonical sparse mutation payloads.
//!
//! This crate is Temnion's scalar typed payload/schema foundation. Schemas are
//! fixed field catalogs keyed by stable numeric identifiers, and sparse
//! mutations encode exact owned scalar values without JSON or Rust-layout
//! coupling.
//!
//! The scope here is intentionally small: scalar `bool`/`i64`/`u64`/`f64` bits,
//! UTF-8 text, raw bytes, per-field nullability/unit metadata, and
//! deterministic little-endian binary encodings for schemas and mutations.
//! Tensor/N-D/vector schemas, schema migrations, field indexes, and
//! model-specific behavior are not implemented yet.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use temnion_core::{FieldId, SchemaId};

/// Maximum number of fields accepted in one schema.
///
/// This is a validation bound, not a preallocation requirement.
pub const MAX_SCHEMA_FIELDS: usize = 4_096;

/// Maximum number of field updates accepted in one sparse upsert.
pub const MAX_MUTATION_FIELDS: usize = MAX_SCHEMA_FIELDS;

/// Maximum UTF-8 byte length for a field name.
pub const MAX_FIELD_NAME_BYTES: usize = 64;

/// Maximum UTF-8 byte length for a field unit string when present.
pub const MAX_FIELD_UNIT_BYTES: usize = 32;

const SCHEMA_ENCODING_VERSION: u8 = 1;
const MUTATION_ENCODING_VERSION: u8 = 1;

const MUTATION_DELETE_TAG: u8 = 0;
const MUTATION_UPSERT_TAG: u8 = 1;

const VALUE_NULL_TAG: u8 = 0;
const VALUE_BOOL_TAG: u8 = 1;
const VALUE_I64_TAG: u8 = 2;
const VALUE_U64_TAG: u8 = 3;
const VALUE_F64_TAG: u8 = 4;
const VALUE_UTF8_TAG: u8 = 5;
const VALUE_BYTES_TAG: u8 = 6;

const FIELD_TYPE_BOOL_TAG: u8 = 0;
const FIELD_TYPE_I64_TAG: u8 = 1;
const FIELD_TYPE_U64_TAG: u8 = 2;
const FIELD_TYPE_F64_TAG: u8 = 3;
const FIELD_TYPE_UTF8_TAG: u8 = 4;
const FIELD_TYPE_BYTES_TAG: u8 = 5;

/// Exact owned scalar values for sparse typed mutations.
///
/// Floating-point values are stored as the exact result of `f64::to_bits()`.
/// Callers recover the floating value with `f64::from_bits()`, preserving
/// bit-identical `-0.0`, infinities, and NaN payloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64Bits(u64),
    Utf8(String),
    Bytes(Vec<u8>),
    Null,
}

impl Value {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::I64(_) => "i64",
            Self::U64(_) => "u64",
            Self::F64Bits(_) => "f64 bits",
            Self::Utf8(_) => "utf8",
            Self::Bytes(_) => "bytes",
            Self::Null => "null",
        }
    }
}

/// Supported scalar field kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Bool,
    I64,
    U64,
    F64,
    Utf8,
    Bytes,
}

impl FieldType {
    fn tag(self) -> u8 {
        match self {
            Self::Bool => FIELD_TYPE_BOOL_TAG,
            Self::I64 => FIELD_TYPE_I64_TAG,
            Self::U64 => FIELD_TYPE_U64_TAG,
            Self::F64 => FIELD_TYPE_F64_TAG,
            Self::Utf8 => FIELD_TYPE_UTF8_TAG,
            Self::Bytes => FIELD_TYPE_BYTES_TAG,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, SchemaError> {
        match tag {
            FIELD_TYPE_BOOL_TAG => Ok(Self::Bool),
            FIELD_TYPE_I64_TAG => Ok(Self::I64),
            FIELD_TYPE_U64_TAG => Ok(Self::U64),
            FIELD_TYPE_F64_TAG => Ok(Self::F64),
            FIELD_TYPE_UTF8_TAG => Ok(Self::Utf8),
            FIELD_TYPE_BYTES_TAG => Ok(Self::Bytes),
            _ => Err(SchemaError::InvalidFieldTypeTag(tag)),
        }
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool => write!(f, "bool"),
            Self::I64 => write!(f, "i64"),
            Self::U64 => write!(f, "u64"),
            Self::F64 => write!(f, "f64"),
            Self::Utf8 => write!(f, "utf8"),
            Self::Bytes => write!(f, "bytes"),
        }
    }
}

/// Fixed schema metadata for one stable field identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: FieldId,
    pub name: String,
    pub kind: FieldType,
    pub nullable: bool,
    pub unit: Option<String>,
}

/// One sparse field change in an upsert mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldUpdate {
    pub field: FieldId,
    pub value: Value,
}

/// Exact typed mutations: sparse upsert or full deletion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mutation {
    Upsert(Vec<FieldUpdate>),
    Delete,
}

/// Validation, decoding, and application errors for scalar schemas and payloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaError {
    AllocationFailed,
    SizeOverflow {
        what: &'static str,
    },
    InvalidSchemaVersion(u32),
    TooManyFields {
        count: usize,
        max: usize,
    },
    TooManyUpdates {
        count: usize,
        max: usize,
    },
    MetadataTooLong {
        what: &'static str,
        len: usize,
        max: usize,
    },
    LengthOutOfRange {
        what: &'static str,
        len: u64,
        max: usize,
    },
    InvalidFieldName(String),
    InvalidFieldUnit(String),
    DuplicateFieldId(FieldId),
    DuplicateFieldName(String),
    UnknownField(FieldId),
    DuplicateMutationField(FieldId),
    EmptyUpsert,
    NullNotAllowed {
        field: FieldId,
    },
    TypeMismatch {
        field: FieldId,
        expected: FieldType,
        actual: &'static str,
    },
    MissingRequiredField(FieldId),
    InputTooLarge {
        len: usize,
        max: usize,
    },
    OutputTooLarge {
        len: usize,
        max: usize,
    },
    InvalidEncodingVersion {
        kind: &'static str,
        version: u8,
    },
    InvalidMutationTag(u8),
    InvalidValueTag(u8),
    InvalidFieldTypeTag(u8),
    InvalidBoolean(u8),
    InvalidUtf8 {
        what: &'static str,
    },
    Truncated {
        needed: usize,
        remaining: usize,
    },
    TrailingBytes(usize),
    NonCanonicalFieldOrder {
        previous: FieldId,
        current: FieldId,
    },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed => write!(f, "could not reserve schema storage"),
            Self::SizeOverflow { what } => write!(f, "{what} size overflowed"),
            Self::InvalidSchemaVersion(version) => {
                write!(f, "schema version must be positive, got {version}")
            }
            Self::TooManyFields { count, max } => {
                write!(f, "schema has {count} fields, max {max}")
            }
            Self::TooManyUpdates { count, max } => {
                write!(f, "mutation has {count} updates, max {max}")
            }
            Self::MetadataTooLong { what, len, max } => {
                write!(f, "{what} length {len} exceeds max {max}")
            }
            Self::LengthOutOfRange { what, len, max } => {
                write!(f, "{what} length {len} exceeds max {max}")
            }
            Self::InvalidFieldName(name) => write!(f, "invalid field name {name:?}"),
            Self::InvalidFieldUnit(unit) => write!(f, "invalid field unit {unit:?}"),
            Self::DuplicateFieldId(field) => write!(f, "duplicate field id {}", field.0),
            Self::DuplicateFieldName(name) => write!(f, "duplicate field name {name:?}"),
            Self::UnknownField(field) => write!(f, "unknown field {}", field.0),
            Self::DuplicateMutationField(field) => {
                write!(f, "mutation updates field {} more than once", field.0)
            }
            Self::EmptyUpsert => write!(f, "an upsert mutation must not be empty"),
            Self::NullNotAllowed { field } => {
                write!(f, "field {} is not nullable", field.0)
            }
            Self::TypeMismatch {
                field,
                expected,
                actual,
            } => write!(f, "field {} expects {expected}, got {actual}", field.0),
            Self::MissingRequiredField(field) => {
                write!(f, "required field {} is missing from state", field.0)
            }
            Self::InputTooLarge { len, max } => {
                write!(f, "input size {len} exceeds max {max}")
            }
            Self::OutputTooLarge { len, max } => {
                write!(f, "encoded size {len} exceeds max {max}")
            }
            Self::InvalidEncodingVersion { kind, version } => {
                write!(f, "unsupported {kind} encoding version {version}")
            }
            Self::InvalidMutationTag(tag) => write!(f, "invalid mutation tag {tag}"),
            Self::InvalidValueTag(tag) => write!(f, "invalid value tag {tag}"),
            Self::InvalidFieldTypeTag(tag) => write!(f, "invalid field type tag {tag}"),
            Self::InvalidBoolean(value) => write!(f, "invalid boolean value {value}"),
            Self::InvalidUtf8 { what } => write!(f, "{what} is not valid utf-8"),
            Self::Truncated { needed, remaining } => write!(
                f,
                "truncated input: needed {needed} more bytes, {remaining} remaining"
            ),
            Self::TrailingBytes(remaining) => write!(f, "{remaining} trailing bytes remain"),
            Self::NonCanonicalFieldOrder { previous, current } => write!(
                f,
                "field ids must be strictly ascending, found {} after {}",
                current.0, previous.0
            ),
        }
    }
}

impl Error for SchemaError {}

/// Immutable scalar schema metadata with deterministic field ordering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    id: SchemaId,
    version: u32,
    fields: Vec<Field>,
}

impl Schema {
    /// Creates a fixed schema with validated metadata and ascending field order.
    pub fn new(id: SchemaId, version: u32, mut fields: Vec<Field>) -> Result<Self, SchemaError> {
        if version == 0 {
            return Err(SchemaError::InvalidSchemaVersion(version));
        }
        if fields.len() > MAX_SCHEMA_FIELDS {
            return Err(SchemaError::TooManyFields {
                count: fields.len(),
                max: MAX_SCHEMA_FIELDS,
            });
        }

        for field in &fields {
            validate_field_name(&field.name)?;
            validate_field_unit(field.unit.as_deref())?;
        }

        fields.sort_unstable_by_key(|field| field.id);
        for pair in fields.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(SchemaError::DuplicateFieldId(pair[1].id));
            }
        }
        for left in 0..fields.len() {
            for right in left + 1..fields.len() {
                if fields[left].name == fields[right].name {
                    return Err(SchemaError::DuplicateFieldName(fields[right].name.clone()));
                }
            }
        }

        Ok(Self {
            id,
            version,
            fields,
        })
    }

    pub fn id(&self) -> SchemaId {
        self.id
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn field(&self, id: FieldId) -> Option<&Field> {
        self.fields
            .binary_search_by_key(&id, |field| field.id)
            .ok()
            .map(|index| &self.fields[index])
    }

    pub fn validate_mutation(&self, mutation: &Mutation) -> Result<(), SchemaError> {
        match mutation {
            Mutation::Delete => Ok(()),
            Mutation::Upsert(updates) => {
                validate_upsert_shape(updates)?;
                for update in updates {
                    let field = self
                        .field(update.field)
                        .ok_or(SchemaError::UnknownField(update.field))?;
                    self.validate_field_value(field, &update.value)?;
                }
                Ok(())
            }
        }
    }

    /// Applies a sparse mutation atomically against a scalar field state map.
    pub fn apply(
        &self,
        state: &mut BTreeMap<FieldId, Value>,
        mutation: &Mutation,
    ) -> Result<(), SchemaError> {
        match mutation {
            Mutation::Delete => {
                state.clear();
                Ok(())
            }
            Mutation::Upsert(updates) => {
                self.validate_mutation(mutation)?;
                let mut next = state.clone();
                for update in updates {
                    next.insert(update.field, update.value.clone());
                }
                self.validate_complete_state(&next)?;
                *state = next;
                Ok(())
            }
        }
    }

    fn validate_field_value(&self, field: &Field, value: &Value) -> Result<(), SchemaError> {
        if matches!(value, Value::Null) {
            if field.nullable {
                return Ok(());
            }
            return Err(SchemaError::NullNotAllowed { field: field.id });
        }

        let matches_kind = matches!(
            (field.kind, value),
            (FieldType::Bool, Value::Bool(_))
                | (FieldType::I64, Value::I64(_))
                | (FieldType::U64, Value::U64(_))
                | (FieldType::F64, Value::F64Bits(_))
                | (FieldType::Utf8, Value::Utf8(_))
                | (FieldType::Bytes, Value::Bytes(_))
        );
        if matches_kind {
            Ok(())
        } else {
            Err(SchemaError::TypeMismatch {
                field: field.id,
                expected: field.kind,
                actual: value.kind_name(),
            })
        }
    }

    fn validate_complete_state(&self, state: &BTreeMap<FieldId, Value>) -> Result<(), SchemaError> {
        for (field_id, value) in state {
            let field = self
                .field(*field_id)
                .ok_or(SchemaError::UnknownField(*field_id))?;
            self.validate_field_value(field, value)?;
        }
        for field in &self.fields {
            if !field.nullable && !state.contains_key(&field.id) {
                return Err(SchemaError::MissingRequiredField(field.id));
            }
        }
        Ok(())
    }
}

/// Encodes a canonical sparse mutation in little-endian binary form.
pub fn encode_mutation(mutation: &Mutation, max_bytes: usize) -> Result<Vec<u8>, SchemaError> {
    let size = mutation_encoded_size(mutation)?;
    if size > max_bytes {
        return Err(SchemaError::OutputTooLarge {
            len: size,
            max: max_bytes,
        });
    }

    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| SchemaError::AllocationFailed)?;
    bytes.push(MUTATION_ENCODING_VERSION);

    match mutation {
        Mutation::Delete => bytes.push(MUTATION_DELETE_TAG),
        Mutation::Upsert(updates) => {
            validate_upsert_shape(updates)?;
            bytes.push(MUTATION_UPSERT_TAG);
            write_u32(
                &mut bytes,
                u32::try_from(updates.len()).map_err(|_| SchemaError::LengthOutOfRange {
                    what: "update count",
                    len: updates.len() as u64,
                    max: u32::MAX as usize,
                })?,
            );
            let mut previous = None;
            for _ in 0..updates.len() {
                let update =
                    next_update_after(updates, previous).ok_or(SchemaError::SizeOverflow {
                        what: "mutation sort",
                    })?;
                write_u16(&mut bytes, update.field.0);
                write_value(&mut bytes, &update.value)?;
                previous = Some(update.field);
            }
        }
    }

    debug_assert_eq!(bytes.len(), size);
    Ok(bytes)
}

/// Decodes a canonical sparse mutation from little-endian binary form.
pub fn decode_mutation(bytes: &[u8], max_bytes: usize) -> Result<Mutation, SchemaError> {
    let mut cursor = Cursor::new(bytes, max_bytes)?;
    let version = cursor.read_u8()?;
    if version != MUTATION_ENCODING_VERSION {
        return Err(SchemaError::InvalidEncodingVersion {
            kind: "mutation",
            version,
        });
    }

    let tag = cursor.read_u8()?;
    let mutation = match tag {
        MUTATION_DELETE_TAG => Mutation::Delete,
        MUTATION_UPSERT_TAG => {
            let raw_count = cursor.read_u32()?;
            let count = usize::try_from(raw_count).map_err(|_| SchemaError::LengthOutOfRange {
                what: "update count",
                len: u64::from(raw_count),
                max: usize::MAX,
            })?;
            if count == 0 {
                return Err(SchemaError::EmptyUpsert);
            }
            if count > MAX_MUTATION_FIELDS {
                return Err(SchemaError::TooManyUpdates {
                    count,
                    max: MAX_MUTATION_FIELDS,
                });
            }
            let mut updates = Vec::new();
            updates
                .try_reserve_exact(count)
                .map_err(|_| SchemaError::AllocationFailed)?;

            let mut previous = None;
            for _ in 0..count {
                let field = FieldId(cursor.read_u16()?);
                if let Some(previous) = previous {
                    if field == previous {
                        return Err(SchemaError::DuplicateMutationField(field));
                    }
                    if field < previous {
                        return Err(SchemaError::NonCanonicalFieldOrder {
                            previous,
                            current: field,
                        });
                    }
                }
                let value = cursor.read_value()?;
                updates.push(FieldUpdate { field, value });
                previous = Some(field);
            }
            Mutation::Upsert(updates)
        }
        _ => return Err(SchemaError::InvalidMutationTag(tag)),
    };

    cursor.finish()?;
    Ok(mutation)
}

/// Encodes a canonical scalar schema in little-endian binary form.
pub fn encode_schema(schema: &Schema, max_bytes: usize) -> Result<Vec<u8>, SchemaError> {
    let size = schema_encoded_size(schema)?;
    if size > max_bytes {
        return Err(SchemaError::OutputTooLarge {
            len: size,
            max: max_bytes,
        });
    }

    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| SchemaError::AllocationFailed)?;
    bytes.push(SCHEMA_ENCODING_VERSION);
    write_u32(&mut bytes, schema.id().0);
    write_u32(&mut bytes, schema.version());
    write_u32(
        &mut bytes,
        u32::try_from(schema.fields().len()).map_err(|_| SchemaError::LengthOutOfRange {
            what: "field count",
            len: schema.fields().len() as u64,
            max: u32::MAX as usize,
        })?,
    );

    for field in schema.fields() {
        write_u16(&mut bytes, field.id.0);
        bytes.push(field.kind.tag());
        bytes.push(u8::from(field.nullable));

        write_u16(
            &mut bytes,
            u16::try_from(field.name.len()).map_err(|_| SchemaError::LengthOutOfRange {
                what: "field name",
                len: field.name.len() as u64,
                max: u16::MAX as usize,
            })?,
        );
        bytes.extend_from_slice(field.name.as_bytes());

        match &field.unit {
            Some(unit) => {
                bytes.push(1);
                write_u16(
                    &mut bytes,
                    u16::try_from(unit.len()).map_err(|_| SchemaError::LengthOutOfRange {
                        what: "field unit",
                        len: unit.len() as u64,
                        max: u16::MAX as usize,
                    })?,
                );
                bytes.extend_from_slice(unit.as_bytes());
            }
            None => bytes.push(0),
        }
    }

    debug_assert_eq!(bytes.len(), size);
    Ok(bytes)
}

/// Decodes a canonical scalar schema from little-endian binary form.
pub fn decode_schema(bytes: &[u8], max_bytes: usize) -> Result<Schema, SchemaError> {
    let mut cursor = Cursor::new(bytes, max_bytes)?;
    let version = cursor.read_u8()?;
    if version != SCHEMA_ENCODING_VERSION {
        return Err(SchemaError::InvalidEncodingVersion {
            kind: "schema",
            version,
        });
    }

    let schema_id = SchemaId(cursor.read_u32()?);
    let schema_version = cursor.read_u32()?;
    let raw_count = cursor.read_u32()?;
    let count = usize::try_from(raw_count).map_err(|_| SchemaError::LengthOutOfRange {
        what: "field count",
        len: u64::from(raw_count),
        max: usize::MAX,
    })?;
    if count > MAX_SCHEMA_FIELDS {
        return Err(SchemaError::TooManyFields {
            count,
            max: MAX_SCHEMA_FIELDS,
        });
    }

    let mut fields = Vec::new();
    fields
        .try_reserve_exact(count)
        .map_err(|_| SchemaError::AllocationFailed)?;

    let mut previous = None;
    for _ in 0..count {
        let id = FieldId(cursor.read_u16()?);
        if let Some(previous) = previous {
            if id == previous {
                return Err(SchemaError::DuplicateFieldId(id));
            }
            if id < previous {
                return Err(SchemaError::NonCanonicalFieldOrder {
                    previous,
                    current: id,
                });
            }
        }

        let kind = FieldType::from_tag(cursor.read_u8()?)?;
        let nullable = cursor.read_bool()?;
        let name_len = cursor.read_u16_len_bounded("field name", MAX_FIELD_NAME_BYTES)?;
        let name = owned_utf8(cursor.take(name_len)?, "field name")?;
        let has_unit = cursor.read_bool()?;
        let unit = if has_unit {
            let unit_len = cursor.read_u16_len_bounded("field unit", MAX_FIELD_UNIT_BYTES)?;
            Some(owned_utf8(cursor.take(unit_len)?, "field unit")?)
        } else {
            None
        };
        fields.push(Field {
            id,
            name,
            kind,
            nullable,
            unit,
        });
        previous = Some(id);
    }

    cursor.finish()?;
    Schema::new(schema_id, schema_version, fields)
}

fn validate_field_name(name: &str) -> Result<(), SchemaError> {
    if name.is_empty() {
        return Err(SchemaError::InvalidFieldName(name.to_owned()));
    }
    if name.len() > MAX_FIELD_NAME_BYTES {
        return Err(SchemaError::MetadataTooLong {
            what: "field name",
            len: name.len(),
            max: MAX_FIELD_NAME_BYTES,
        });
    }

    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(SchemaError::InvalidFieldName(name.to_owned()));
    };
    if !matches!(first, 'A'..='Z' | 'a'..='z' | '_') {
        return Err(SchemaError::InvalidFieldName(name.to_owned()));
    }
    if chars.any(|ch| !matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-' | '.')) {
        return Err(SchemaError::InvalidFieldName(name.to_owned()));
    }
    Ok(())
}

fn validate_field_unit(unit: Option<&str>) -> Result<(), SchemaError> {
    let Some(unit) = unit else {
        return Ok(());
    };
    if unit.is_empty() || unit.trim() != unit || unit.chars().any(|ch| ch.is_control()) {
        return Err(SchemaError::InvalidFieldUnit(unit.to_owned()));
    }
    if unit.len() > MAX_FIELD_UNIT_BYTES {
        return Err(SchemaError::MetadataTooLong {
            what: "field unit",
            len: unit.len(),
            max: MAX_FIELD_UNIT_BYTES,
        });
    }
    Ok(())
}

fn validate_upsert_shape(updates: &[FieldUpdate]) -> Result<(), SchemaError> {
    if updates.is_empty() {
        return Err(SchemaError::EmptyUpsert);
    }
    if updates.len() > MAX_MUTATION_FIELDS {
        return Err(SchemaError::TooManyUpdates {
            count: updates.len(),
            max: MAX_MUTATION_FIELDS,
        });
    }
    for left in 0..updates.len() {
        for right in left + 1..updates.len() {
            if updates[left].field == updates[right].field {
                return Err(SchemaError::DuplicateMutationField(updates[right].field));
            }
        }
    }
    Ok(())
}

fn next_update_after(updates: &[FieldUpdate], previous: Option<FieldId>) -> Option<&FieldUpdate> {
    let mut best: Option<&FieldUpdate> = None;
    for update in updates {
        if previous.is_some_and(|previous| update.field <= previous) {
            continue;
        }
        if best.is_none_or(|best| update.field < best.field) {
            best = Some(update);
        }
    }
    best
}

fn mutation_encoded_size(mutation: &Mutation) -> Result<usize, SchemaError> {
    let mut size = 2usize;
    match mutation {
        Mutation::Delete => Ok(size),
        Mutation::Upsert(updates) => {
            validate_upsert_shape(updates)?;
            size = checked_add_size(size, 4, "mutation encoding")?;
            for update in updates {
                size = checked_add_size(size, 2, "mutation encoding")?;
                size = checked_add_size(
                    size,
                    value_encoded_size(&update.value)?,
                    "mutation encoding",
                )?;
            }
            Ok(size)
        }
    }
}

fn schema_encoded_size(schema: &Schema) -> Result<usize, SchemaError> {
    if schema.fields().len() > MAX_SCHEMA_FIELDS {
        return Err(SchemaError::TooManyFields {
            count: schema.fields().len(),
            max: MAX_SCHEMA_FIELDS,
        });
    }

    let mut size = 13usize;
    for field in schema.fields() {
        size = checked_add_size(size, 2, "schema encoding")?;
        size = checked_add_size(size, 1, "schema encoding")?;
        size = checked_add_size(size, 1, "schema encoding")?;
        size = checked_add_size(size, 2, "schema encoding")?;
        size = checked_add_size(size, field.name.len(), "schema encoding")?;
        size = checked_add_size(size, 1, "schema encoding")?;
        if let Some(unit) = &field.unit {
            size = checked_add_size(size, 2, "schema encoding")?;
            size = checked_add_size(size, unit.len(), "schema encoding")?;
        }
    }
    Ok(size)
}

fn value_encoded_size(value: &Value) -> Result<usize, SchemaError> {
    match value {
        Value::Null => Ok(1),
        Value::Bool(_) => Ok(2),
        Value::I64(_) | Value::U64(_) | Value::F64Bits(_) => Ok(9),
        Value::Utf8(text) => {
            if text.len() > u32::MAX as usize {
                return Err(SchemaError::LengthOutOfRange {
                    what: "utf8 value",
                    len: text.len() as u64,
                    max: u32::MAX as usize,
                });
            }
            checked_add_size(5, text.len(), "value encoding")
        }
        Value::Bytes(bytes) => {
            if bytes.len() > u32::MAX as usize {
                return Err(SchemaError::LengthOutOfRange {
                    what: "byte value",
                    len: bytes.len() as u64,
                    max: u32::MAX as usize,
                });
            }
            checked_add_size(5, bytes.len(), "value encoding")
        }
    }
}

fn checked_add_size(total: usize, add: usize, what: &'static str) -> Result<usize, SchemaError> {
    total
        .checked_add(add)
        .ok_or(SchemaError::SizeOverflow { what })
}

fn write_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_value(bytes: &mut Vec<u8>, value: &Value) -> Result<(), SchemaError> {
    match value {
        Value::Null => bytes.push(VALUE_NULL_TAG),
        Value::Bool(value) => {
            bytes.push(VALUE_BOOL_TAG);
            bytes.push(u8::from(*value));
        }
        Value::I64(value) => {
            bytes.push(VALUE_I64_TAG);
            write_i64(bytes, *value);
        }
        Value::U64(value) => {
            bytes.push(VALUE_U64_TAG);
            write_u64(bytes, *value);
        }
        Value::F64Bits(value) => {
            bytes.push(VALUE_F64_TAG);
            write_u64(bytes, *value);
        }
        Value::Utf8(text) => {
            bytes.push(VALUE_UTF8_TAG);
            write_u32(
                bytes,
                u32::try_from(text.len()).map_err(|_| SchemaError::LengthOutOfRange {
                    what: "utf8 value",
                    len: text.len() as u64,
                    max: u32::MAX as usize,
                })?,
            );
            bytes.extend_from_slice(text.as_bytes());
        }
        Value::Bytes(value) => {
            bytes.push(VALUE_BYTES_TAG);
            write_u32(
                bytes,
                u32::try_from(value.len()).map_err(|_| SchemaError::LengthOutOfRange {
                    what: "byte value",
                    len: value.len() as u64,
                    max: u32::MAX as usize,
                })?,
            );
            bytes.extend_from_slice(value);
        }
    }
    Ok(())
}

fn owned_utf8(bytes: &[u8], what: &'static str) -> Result<String, SchemaError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SchemaError::InvalidUtf8 { what })?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| SchemaError::AllocationFailed)?;
    owned.push_str(text);
    Ok(owned)
}

fn owned_bytes(bytes: &[u8]) -> Result<Vec<u8>, SchemaError> {
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(bytes.len())
        .map_err(|_| SchemaError::AllocationFailed)?;
    owned.extend_from_slice(bytes);
    Ok(owned)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    max_bytes: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], max_bytes: usize) -> Result<Self, SchemaError> {
        if bytes.len() > max_bytes {
            return Err(SchemaError::InputTooLarge {
                len: bytes.len(),
                max: max_bytes,
            });
        }
        Ok(Self {
            bytes,
            position: 0,
            max_bytes,
        })
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn finish(self) -> Result<(), SchemaError> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(SchemaError::TrailingBytes(self.bytes.len() - self.position))
        }
    }

    fn read_u8(&mut self) -> Result<u8, SchemaError> {
        Ok(self.take(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, SchemaError> {
        let slice = self.take(2)?;
        Ok(u16::from_le_bytes([slice[0], slice[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, SchemaError> {
        let slice = self.take(4)?;
        Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn read_u64(&mut self) -> Result<u64, SchemaError> {
        let slice = self.take(8)?;
        Ok(u64::from_le_bytes([
            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
        ]))
    }

    fn read_i64(&mut self) -> Result<i64, SchemaError> {
        let slice = self.take(8)?;
        Ok(i64::from_le_bytes([
            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
        ]))
    }

    fn read_bool(&mut self) -> Result<bool, SchemaError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(SchemaError::InvalidBoolean(value)),
        }
    }

    fn read_u16_len_bounded(
        &mut self,
        what: &'static str,
        max: usize,
    ) -> Result<usize, SchemaError> {
        let len = usize::from(self.read_u16()?);
        if len > max {
            return Err(SchemaError::MetadataTooLong { what, len, max });
        }
        Ok(len)
    }

    fn read_u32_len_bounded(
        &mut self,
        what: &'static str,
        max: usize,
    ) -> Result<usize, SchemaError> {
        let raw = self.read_u32()?;
        let len = usize::try_from(raw).map_err(|_| SchemaError::LengthOutOfRange {
            what,
            len: u64::from(raw),
            max,
        })?;
        if len > max {
            return Err(SchemaError::LengthOutOfRange {
                what,
                len: u64::from(raw),
                max,
            });
        }
        Ok(len)
    }

    fn read_value(&mut self) -> Result<Value, SchemaError> {
        let tag = self.read_u8()?;
        match tag {
            VALUE_NULL_TAG => Ok(Value::Null),
            VALUE_BOOL_TAG => Ok(Value::Bool(self.read_bool()?)),
            VALUE_I64_TAG => Ok(Value::I64(self.read_i64()?)),
            VALUE_U64_TAG => Ok(Value::U64(self.read_u64()?)),
            VALUE_F64_TAG => Ok(Value::F64Bits(self.read_u64()?)),
            VALUE_UTF8_TAG => {
                let len = self.read_u32_len_bounded("utf8 value", self.max_bytes)?;
                Ok(Value::Utf8(owned_utf8(self.take(len)?, "utf8 value")?))
            }
            VALUE_BYTES_TAG => {
                let len = self.read_u32_len_bounded("byte value", self.max_bytes)?;
                Ok(Value::Bytes(owned_bytes(self.take(len)?)?))
            }
            _ => Err(SchemaError::InvalidValueTag(tag)),
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], SchemaError> {
        let end = self
            .position
            .checked_add(len)
            .ok_or(SchemaError::SizeOverflow {
                what: "decode cursor",
            })?;
        if end > self.bytes.len() {
            return Err(SchemaError::Truncated {
                needed: len,
                remaining: self.remaining(),
            });
        }
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(id: u16, name: &str, kind: FieldType, nullable: bool, unit: Option<&str>) -> Field {
        Field {
            id: FieldId(id),
            name: name.to_owned(),
            kind,
            nullable,
            unit: unit.map(str::to_owned),
        }
    }

    fn bool_entry(field_id: u16, value: bool) -> Vec<u8> {
        vec![
            field_id as u8,
            (field_id >> 8) as u8,
            VALUE_BOOL_TAG,
            u8::from(value),
        ]
    }

    fn simple_schema_field_bytes(field_id: u16, name: u8) -> Vec<u8> {
        vec![
            field_id as u8,
            (field_id >> 8) as u8,
            FIELD_TYPE_BOOL_TAG,
            0,
            1,
            0,
            name,
            0,
        ]
    }

    #[test]
    fn schema_new_sorts_fields_and_accessors_are_stable() {
        let schema = Schema::new(
            SchemaId(7),
            3,
            vec![
                field(7, "label", FieldType::Utf8, true, None),
                field(1, "active", FieldType::Bool, false, None),
                field(3, "elapsed_ms", FieldType::U64, false, Some("ms")),
            ],
        )
        .unwrap();

        assert_eq!(schema.id(), SchemaId(7));
        assert_eq!(schema.version(), 3);
        assert_eq!(
            schema
                .fields()
                .iter()
                .map(|field| field.id)
                .collect::<Vec<_>>(),
            vec![FieldId(1), FieldId(3), FieldId(7)]
        );
        assert_eq!(
            schema.field(FieldId(3)).map(|field| field.name.as_str()),
            Some("elapsed_ms")
        );
        assert!(schema.field(FieldId(99)).is_none());
    }

    #[test]
    fn schema_new_rejects_invalid_metadata_and_duplicates() {
        assert_eq!(
            Schema::new(SchemaId(1), 0, vec![]),
            Err(SchemaError::InvalidSchemaVersion(0))
        );
        assert!(matches!(
            Schema::new(
                SchemaId(1),
                1,
                vec![
                    field(1, "ok", FieldType::Bool, false, None),
                    field(2, "bad name", FieldType::Bool, false, None)
                ]
            ),
            Err(SchemaError::InvalidFieldName(_))
        ));
        assert!(matches!(
            Schema::new(
                SchemaId(1),
                1,
                vec![
                    field(1, "ok", FieldType::Bool, false, None),
                    field(2, "also_ok", FieldType::Bool, false, Some(" ms"))
                ]
            ),
            Err(SchemaError::InvalidFieldUnit(_))
        ));
        assert_eq!(
            Schema::new(
                SchemaId(1),
                1,
                vec![
                    field(1, "one", FieldType::Bool, false, None),
                    field(1, "two", FieldType::U64, false, None)
                ]
            ),
            Err(SchemaError::DuplicateFieldId(FieldId(1)))
        );
        assert_eq!(
            Schema::new(
                SchemaId(1),
                1,
                vec![
                    field(1, "same", FieldType::Bool, false, None),
                    field(2, "same", FieldType::U64, false, None)
                ]
            ),
            Err(SchemaError::DuplicateFieldName("same".to_owned()))
        );

        let long_name = "a".repeat(MAX_FIELD_NAME_BYTES + 1);
        assert_eq!(
            Schema::new(
                SchemaId(1),
                1,
                vec![Field {
                    id: FieldId(1),
                    name: long_name.clone(),
                    kind: FieldType::Bool,
                    nullable: false,
                    unit: None,
                }]
            ),
            Err(SchemaError::MetadataTooLong {
                what: "field name",
                len: long_name.len(),
                max: MAX_FIELD_NAME_BYTES,
            })
        );

        let long_unit = "u".repeat(MAX_FIELD_UNIT_BYTES + 1);
        assert_eq!(
            Schema::new(
                SchemaId(1),
                1,
                vec![Field {
                    id: FieldId(1),
                    name: "valid".to_owned(),
                    kind: FieldType::Bool,
                    nullable: false,
                    unit: Some(long_unit.clone()),
                }]
            ),
            Err(SchemaError::MetadataTooLong {
                what: "field unit",
                len: long_unit.len(),
                max: MAX_FIELD_UNIT_BYTES,
            })
        );
    }

    #[test]
    fn validate_mutation_checks_types_nullability_duplicates_and_unknown_fields() {
        let schema = Schema::new(
            SchemaId(2),
            1,
            vec![
                field(1, "active", FieldType::Bool, false, None),
                field(2, "payload", FieldType::Bytes, true, Some("B")),
            ],
        )
        .unwrap();

        assert_eq!(
            schema.validate_mutation(&Mutation::Upsert(vec![])),
            Err(SchemaError::EmptyUpsert)
        );
        assert_eq!(
            schema.validate_mutation(&Mutation::Upsert(vec![
                FieldUpdate {
                    field: FieldId(1),
                    value: Value::Bool(true),
                },
                FieldUpdate {
                    field: FieldId(1),
                    value: Value::Bool(false),
                },
            ])),
            Err(SchemaError::DuplicateMutationField(FieldId(1)))
        );
        assert_eq!(
            schema.validate_mutation(&Mutation::Upsert(vec![FieldUpdate {
                field: FieldId(99),
                value: Value::Bool(true),
            }])),
            Err(SchemaError::UnknownField(FieldId(99)))
        );
        assert_eq!(
            schema.validate_mutation(&Mutation::Upsert(vec![FieldUpdate {
                field: FieldId(1),
                value: Value::Utf8("nope".to_owned()),
            }])),
            Err(SchemaError::TypeMismatch {
                field: FieldId(1),
                expected: FieldType::Bool,
                actual: "utf8",
            })
        );
        assert_eq!(
            schema.validate_mutation(&Mutation::Upsert(vec![FieldUpdate {
                field: FieldId(1),
                value: Value::Null,
            }])),
            Err(SchemaError::NullNotAllowed { field: FieldId(1) })
        );
    }

    #[test]
    fn apply_requires_complete_required_state_supports_sparse_updates_and_delete() {
        let schema = Schema::new(
            SchemaId(3),
            1,
            vec![
                field(1, "active", FieldType::Bool, false, None),
                field(2, "count", FieldType::U64, false, Some("items")),
                field(3, "note", FieldType::Utf8, true, None),
            ],
        )
        .unwrap();

        let mut state = BTreeMap::new();
        let missing = schema.apply(
            &mut state,
            &Mutation::Upsert(vec![FieldUpdate {
                field: FieldId(1),
                value: Value::Bool(true),
            }]),
        );
        assert_eq!(missing, Err(SchemaError::MissingRequiredField(FieldId(2))));
        assert!(state.is_empty());

        schema
            .apply(
                &mut state,
                &Mutation::Upsert(vec![
                    FieldUpdate {
                        field: FieldId(2),
                        value: Value::U64(9),
                    },
                    FieldUpdate {
                        field: FieldId(1),
                        value: Value::Bool(true),
                    },
                ]),
            )
            .unwrap();
        assert_eq!(state.get(&FieldId(1)), Some(&Value::Bool(true)));
        assert_eq!(state.get(&FieldId(2)), Some(&Value::U64(9)));
        assert!(!state.contains_key(&FieldId(3)));

        schema
            .apply(
                &mut state,
                &Mutation::Upsert(vec![FieldUpdate {
                    field: FieldId(3),
                    value: Value::Utf8("héllo".to_owned()),
                }]),
            )
            .unwrap();
        assert_eq!(
            state.get(&FieldId(3)),
            Some(&Value::Utf8("héllo".to_owned()))
        );
        assert_eq!(state.len(), 3);

        schema.apply(&mut state, &Mutation::Delete).unwrap();
        assert!(state.is_empty());
    }

    #[test]
    fn apply_is_atomic_when_final_state_validation_fails() {
        let schema = Schema::new(
            SchemaId(4),
            1,
            vec![field(1, "active", FieldType::Bool, false, None)],
        )
        .unwrap();

        let mut state = BTreeMap::from([
            (FieldId(1), Value::Bool(true)),
            (FieldId(99), Value::U64(7)),
        ]);
        let before = state.clone();
        let result = schema.apply(
            &mut state,
            &Mutation::Upsert(vec![FieldUpdate {
                field: FieldId(1),
                value: Value::Bool(false),
            }]),
        );

        assert_eq!(result, Err(SchemaError::UnknownField(FieldId(99))));
        assert_eq!(state, before);
    }

    #[test]
    fn mutation_roundtrip_preserves_scalar_extremes_unicode_and_bit_exact_floats() {
        let mutation = Mutation::Upsert(vec![
            FieldUpdate {
                field: FieldId(12),
                value: Value::Null,
            },
            FieldUpdate {
                field: FieldId(2),
                value: Value::I64(i64::MIN),
            },
            FieldUpdate {
                field: FieldId(10),
                value: Value::F64Bits(0x7ff0_0000_0000_0001),
            },
            FieldUpdate {
                field: FieldId(4),
                value: Value::F64Bits(0.0f64.to_bits()),
            },
            FieldUpdate {
                field: FieldId(5),
                value: Value::F64Bits((-0.0f64).to_bits()),
            },
            FieldUpdate {
                field: FieldId(9),
                value: Value::Utf8("héllo 🚀".to_owned()),
            },
            FieldUpdate {
                field: FieldId(8),
                value: Value::Bytes(vec![0, 1, 2, 127, 128, 255]),
            },
            FieldUpdate {
                field: FieldId(1),
                value: Value::Bool(true),
            },
            FieldUpdate {
                field: FieldId(6),
                value: Value::F64Bits(0x7ff8_0000_0000_0001),
            },
            FieldUpdate {
                field: FieldId(3),
                value: Value::U64(u64::MAX),
            },
            FieldUpdate {
                field: FieldId(11),
                value: Value::I64(i64::MAX),
            },
            FieldUpdate {
                field: FieldId(7),
                value: Value::F64Bits(f64::INFINITY.to_bits()),
            },
            FieldUpdate {
                field: FieldId(13),
                value: Value::Bool(false),
            },
            FieldUpdate {
                field: FieldId(14),
                value: Value::U64(0),
            },
        ]);

        let encoded = encode_mutation(&mutation, 4 * 1024).unwrap();
        let decoded = decode_mutation(&encoded, encoded.len()).unwrap();

        assert_eq!(
            decoded,
            Mutation::Upsert(vec![
                FieldUpdate {
                    field: FieldId(1),
                    value: Value::Bool(true),
                },
                FieldUpdate {
                    field: FieldId(2),
                    value: Value::I64(i64::MIN),
                },
                FieldUpdate {
                    field: FieldId(3),
                    value: Value::U64(u64::MAX),
                },
                FieldUpdate {
                    field: FieldId(4),
                    value: Value::F64Bits(0.0f64.to_bits()),
                },
                FieldUpdate {
                    field: FieldId(5),
                    value: Value::F64Bits((-0.0f64).to_bits()),
                },
                FieldUpdate {
                    field: FieldId(6),
                    value: Value::F64Bits(0x7ff8_0000_0000_0001),
                },
                FieldUpdate {
                    field: FieldId(7),
                    value: Value::F64Bits(f64::INFINITY.to_bits()),
                },
                FieldUpdate {
                    field: FieldId(8),
                    value: Value::Bytes(vec![0, 1, 2, 127, 128, 255]),
                },
                FieldUpdate {
                    field: FieldId(9),
                    value: Value::Utf8("héllo 🚀".to_owned()),
                },
                FieldUpdate {
                    field: FieldId(10),
                    value: Value::F64Bits(0x7ff0_0000_0000_0001),
                },
                FieldUpdate {
                    field: FieldId(11),
                    value: Value::I64(i64::MAX),
                },
                FieldUpdate {
                    field: FieldId(12),
                    value: Value::Null,
                },
                FieldUpdate {
                    field: FieldId(13),
                    value: Value::Bool(false),
                },
                FieldUpdate {
                    field: FieldId(14),
                    value: Value::U64(0),
                },
            ])
        );
    }

    #[test]
    fn mutation_and_schema_encoding_are_deterministic_across_input_order() {
        let left_schema = Schema::new(
            SchemaId(5),
            2,
            vec![
                field(3, "count", FieldType::U64, false, Some("items")),
                field(6, "payload", FieldType::Bytes, true, Some("B")),
                field(2, "delta", FieldType::I64, true, None),
                field(1, "active", FieldType::Bool, false, None),
                field(4, "temperature", FieldType::F64, false, Some("°C")),
                field(5, "note", FieldType::Utf8, true, None),
            ],
        )
        .unwrap();
        let right_schema = Schema::new(
            SchemaId(5),
            2,
            vec![
                field(5, "note", FieldType::Utf8, true, None),
                field(2, "delta", FieldType::I64, true, None),
                field(6, "payload", FieldType::Bytes, true, Some("B")),
                field(1, "active", FieldType::Bool, false, None),
                field(3, "count", FieldType::U64, false, Some("items")),
                field(4, "temperature", FieldType::F64, false, Some("°C")),
            ],
        )
        .unwrap();
        let left_schema_bytes = encode_schema(&left_schema, 1024).unwrap();
        let right_schema_bytes = encode_schema(&right_schema, 1024).unwrap();
        assert_eq!(left_schema_bytes, right_schema_bytes);
        assert_eq!(
            decode_schema(&left_schema_bytes, left_schema_bytes.len()).unwrap(),
            left_schema
        );

        let left_mutation = Mutation::Upsert(vec![
            FieldUpdate {
                field: FieldId(3),
                value: Value::U64(5),
            },
            FieldUpdate {
                field: FieldId(1),
                value: Value::Bool(true),
            },
        ]);
        let right_mutation = Mutation::Upsert(vec![
            FieldUpdate {
                field: FieldId(1),
                value: Value::Bool(true),
            },
            FieldUpdate {
                field: FieldId(3),
                value: Value::U64(5),
            },
        ]);
        assert_eq!(
            encode_mutation(&left_mutation, 256).unwrap(),
            encode_mutation(&right_mutation, 256).unwrap()
        );
    }

    #[test]
    fn decode_mutation_rejects_every_truncation_prefix() {
        let bytes = encode_mutation(
            &Mutation::Upsert(vec![
                FieldUpdate {
                    field: FieldId(2),
                    value: Value::Utf8("μs".to_owned()),
                },
                FieldUpdate {
                    field: FieldId(1),
                    value: Value::Bytes(vec![1, 2, 3]),
                },
            ]),
            512,
        )
        .unwrap();

        for end in 0..bytes.len() {
            assert!(
                decode_mutation(&bytes[..end], bytes.len()).is_err(),
                "prefix {end}"
            );
        }
        assert!(decode_mutation(&bytes, bytes.len()).is_ok());
    }

    #[test]
    fn decode_schema_rejects_every_truncation_prefix() {
        let schema = Schema::new(
            SchemaId(6),
            1,
            vec![
                field(2, "payload", FieldType::Bytes, true, Some("B")),
                field(1, "active", FieldType::Bool, false, None),
            ],
        )
        .unwrap();
        let bytes = encode_schema(&schema, 512).unwrap();

        for end in 0..bytes.len() {
            assert!(
                decode_schema(&bytes[..end], bytes.len()).is_err(),
                "prefix {end}"
            );
        }
        assert!(decode_schema(&bytes, bytes.len()).is_ok());
    }

    #[test]
    fn decode_rejects_unknown_versions_tags_booleans_utf8_and_trailing_bytes() {
        assert_eq!(
            decode_mutation(&[2, MUTATION_DELETE_TAG], 2),
            Err(SchemaError::InvalidEncodingVersion {
                kind: "mutation",
                version: 2,
            })
        );
        assert_eq!(
            decode_mutation(&[MUTATION_ENCODING_VERSION, 9], 2),
            Err(SchemaError::InvalidMutationTag(9))
        );
        assert_eq!(
            decode_mutation(
                &[
                    MUTATION_ENCODING_VERSION,
                    MUTATION_UPSERT_TAG,
                    1,
                    0,
                    0,
                    0,
                    1,
                    0,
                    VALUE_BOOL_TAG,
                    2,
                ],
                32
            ),
            Err(SchemaError::InvalidBoolean(2))
        );
        assert_eq!(
            decode_mutation(
                &[
                    MUTATION_ENCODING_VERSION,
                    MUTATION_UPSERT_TAG,
                    1,
                    0,
                    0,
                    0,
                    1,
                    0,
                    99,
                ],
                32
            ),
            Err(SchemaError::InvalidValueTag(99))
        );
        assert_eq!(
            decode_mutation(
                &[
                    MUTATION_ENCODING_VERSION,
                    MUTATION_UPSERT_TAG,
                    1,
                    0,
                    0,
                    0,
                    1,
                    0,
                    VALUE_UTF8_TAG,
                    1,
                    0,
                    0,
                    0,
                    0xff,
                ],
                32
            ),
            Err(SchemaError::InvalidUtf8 { what: "utf8 value" })
        );
        assert_eq!(
            decode_mutation(&[MUTATION_ENCODING_VERSION, MUTATION_DELETE_TAG, 0], 8),
            Err(SchemaError::TrailingBytes(1))
        );

        let schema_header = [SCHEMA_ENCODING_VERSION, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0];
        let mut invalid_type = schema_header.to_vec();
        invalid_type.extend_from_slice(&[1, 0, 9, 0, 1, 0, b'a', 0]);
        assert_eq!(
            decode_schema(&invalid_type, 64),
            Err(SchemaError::InvalidFieldTypeTag(9))
        );

        let mut invalid_nullable = schema_header.to_vec();
        invalid_nullable.extend_from_slice(&[1, 0, FIELD_TYPE_BOOL_TAG, 2, 1, 0, b'a', 0]);
        assert_eq!(
            decode_schema(&invalid_nullable, 64),
            Err(SchemaError::InvalidBoolean(2))
        );

        let mut invalid_unit_flag = schema_header.to_vec();
        invalid_unit_flag.extend_from_slice(&[1, 0, FIELD_TYPE_BOOL_TAG, 0, 1, 0, b'a', 2]);
        assert_eq!(
            decode_schema(&invalid_unit_flag, 64),
            Err(SchemaError::InvalidBoolean(2))
        );

        let mut invalid_name_utf8 = schema_header.to_vec();
        invalid_name_utf8.extend_from_slice(&[1, 0, FIELD_TYPE_BOOL_TAG, 0, 1, 0, 0xff, 0]);
        assert_eq!(
            decode_schema(&invalid_name_utf8, 64),
            Err(SchemaError::InvalidUtf8 { what: "field name" })
        );

        let valid_schema = encode_schema(
            &Schema::new(
                SchemaId(1),
                1,
                vec![field(1, "a", FieldType::Bool, false, None)],
            )
            .unwrap(),
            128,
        )
        .unwrap();
        let mut with_trailing = valid_schema.clone();
        with_trailing.push(0);
        assert_eq!(
            decode_schema(&with_trailing, 128),
            Err(SchemaError::TrailingBytes(1))
        );
        assert_eq!(
            decode_schema(&[2], 1),
            Err(SchemaError::InvalidEncodingVersion {
                kind: "schema",
                version: 2,
            })
        );
    }

    #[test]
    fn decode_rejects_noncanonical_order_duplicates_and_bounded_huge_lengths() {
        let mut noncanonical_mutation =
            vec![MUTATION_ENCODING_VERSION, MUTATION_UPSERT_TAG, 2, 0, 0, 0];
        noncanonical_mutation.extend_from_slice(&bool_entry(2, true));
        noncanonical_mutation.extend_from_slice(&bool_entry(1, false));
        assert_eq!(
            decode_mutation(&noncanonical_mutation, 64),
            Err(SchemaError::NonCanonicalFieldOrder {
                previous: FieldId(2),
                current: FieldId(1),
            })
        );

        let mut duplicate_mutation =
            vec![MUTATION_ENCODING_VERSION, MUTATION_UPSERT_TAG, 2, 0, 0, 0];
        duplicate_mutation.extend_from_slice(&bool_entry(2, true));
        duplicate_mutation.extend_from_slice(&bool_entry(2, false));
        assert_eq!(
            decode_mutation(&duplicate_mutation, 64),
            Err(SchemaError::DuplicateMutationField(FieldId(2)))
        );

        let mut noncanonical_schema =
            vec![SCHEMA_ENCODING_VERSION, 1, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0];
        noncanonical_schema.extend_from_slice(&simple_schema_field_bytes(2, b'b'));
        noncanonical_schema.extend_from_slice(&simple_schema_field_bytes(1, b'a'));
        assert_eq!(
            decode_schema(&noncanonical_schema, 128),
            Err(SchemaError::NonCanonicalFieldOrder {
                previous: FieldId(2),
                current: FieldId(1),
            })
        );

        let mut duplicate_schema =
            vec![SCHEMA_ENCODING_VERSION, 1, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0];
        duplicate_schema.extend_from_slice(&simple_schema_field_bytes(2, b'b'));
        duplicate_schema.extend_from_slice(&simple_schema_field_bytes(2, b'c'));
        assert_eq!(
            decode_schema(&duplicate_schema, 128),
            Err(SchemaError::DuplicateFieldId(FieldId(2)))
        );

        assert_eq!(
            decode_mutation(
                &[
                    MUTATION_ENCODING_VERSION,
                    MUTATION_UPSERT_TAG,
                    1,
                    0,
                    0,
                    0,
                    1,
                    0,
                    VALUE_BYTES_TAG,
                    0xff,
                    0xff,
                    0xff,
                    0xff,
                ],
                64
            ),
            Err(SchemaError::LengthOutOfRange {
                what: "byte value",
                len: u32::MAX as u64,
                max: 64,
            })
        );
        assert_eq!(
            decode_mutation(
                &[MUTATION_ENCODING_VERSION, MUTATION_UPSERT_TAG, 1, 16, 0, 0],
                64
            ),
            Err(SchemaError::TooManyUpdates {
                count: MAX_MUTATION_FIELDS + 1,
                max: MAX_MUTATION_FIELDS,
            })
        );

        let mut huge_name_schema = schema_header_for_one_field();
        huge_name_schema.extend_from_slice(&[1, 0, FIELD_TYPE_BOOL_TAG, 0, 65, 0]);
        assert_eq!(
            decode_schema(&huge_name_schema, 128),
            Err(SchemaError::MetadataTooLong {
                what: "field name",
                len: 65,
                max: MAX_FIELD_NAME_BYTES,
            })
        );
        assert_eq!(
            decode_schema(
                &[SCHEMA_ENCODING_VERSION, 1, 0, 0, 0, 1, 0, 0, 0, 1, 16, 0, 0],
                64
            ),
            Err(SchemaError::TooManyFields {
                count: MAX_SCHEMA_FIELDS + 1,
                max: MAX_SCHEMA_FIELDS,
            })
        );
    }

    #[test]
    fn encode_and_decode_respect_max_byte_limits() {
        let encoded_delete = encode_mutation(&Mutation::Delete, 16).unwrap();
        assert_eq!(
            decode_mutation(&encoded_delete, encoded_delete.len()).unwrap(),
            Mutation::Delete
        );

        let mutation = Mutation::Upsert(vec![FieldUpdate {
            field: FieldId(1),
            value: Value::Utf8("hello".to_owned()),
        }]);
        let encoded_mutation = encode_mutation(&mutation, 128).unwrap();
        assert_eq!(
            encode_mutation(&mutation, encoded_mutation.len() - 1),
            Err(SchemaError::OutputTooLarge {
                len: encoded_mutation.len(),
                max: encoded_mutation.len() - 1,
            })
        );
        assert_eq!(
            decode_mutation(&encoded_mutation, encoded_mutation.len() - 1),
            Err(SchemaError::InputTooLarge {
                len: encoded_mutation.len(),
                max: encoded_mutation.len() - 1,
            })
        );

        let schema = Schema::new(
            SchemaId(8),
            1,
            vec![field(1, "active", FieldType::Bool, false, None)],
        )
        .unwrap();
        let encoded_schema = encode_schema(&schema, 128).unwrap();
        assert_eq!(
            encode_schema(&schema, encoded_schema.len() - 1),
            Err(SchemaError::OutputTooLarge {
                len: encoded_schema.len(),
                max: encoded_schema.len() - 1,
            })
        );
        assert_eq!(
            decode_schema(&encoded_schema, encoded_schema.len() - 1),
            Err(SchemaError::InputTooLarge {
                len: encoded_schema.len(),
                max: encoded_schema.len() - 1,
            })
        );
    }

    fn schema_header_for_one_field() -> Vec<u8> {
        vec![SCHEMA_ENCODING_VERSION, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]
    }
}
