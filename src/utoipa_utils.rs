//! Conversion utilities for converting between utoipa and open-rpc schema types.
//!
//! This module provides a `TryFrom` implementation to convert from `utoipa::openapi::Schema`
//! to `open_rpc::Schema`. The conversion handles most common schema types including:
//!
//! - Primitive types (string, integer, number, boolean, null)
//! - Complex types (object, array)
//! - Composite types (oneOf, allOf, anyOf)
//! - Schema references
//!
//! ## Example
//!
//! ```rust
//! use utoipa::openapi::schema::{ObjectBuilder, Type};
//! use open_rpc::Schema;
//!
//! let utoipa_schema = utoipa::openapi::Schema::Object(
//!     ObjectBuilder::new()
//!         .schema_type(Type::String)
//!         .title(Some("Example".to_string()))
//!         .build()
//! );
//!
//! let open_rpc_schema = Schema::try_from(utoipa_schema).unwrap();
//! ```
//!
//! ## Limitations
//!
//! Some utoipa features are not supported and will result in conversion errors:
//!
//! - Multiple types in a single schema (e.g., `["string", "number"]`)
//! - Certain string formats that don't have direct equivalents
//! - Custom string formats
//! - Some conditionally compiled features in utoipa

use crate::{
    ArrayLiteral, IntegerLiteral, Literal, NumberLiteral, ObjectLiteral, Schema, SchemaContents,
    StringFormat, StringLiteral,
};
use std::collections::BTreeMap;
use utoipa::openapi::{
    schema::{KnownFormat, SchemaFormat, SchemaType, Type},
    RefOr,
};

pub use utoipa;

/// Errors that can occur during schema conversion.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Schema reference conversion is not supported.
    #[error("Unsupported schema reference conversion")]
    UnsupportedReference,

    /// The utoipa schema format has no equivalent in open-rpc.
    #[error("Unsupported schema format")]
    UnsupportedFormat,

    /// The utoipa schema type is not supported for conversion.
    #[error("Unsupported schema type")]
    UnsupportedSchemaType,

    /// Array items type is not supported.
    #[error("Unsupported array items type")]
    UnsupportedArrayItems,

    /// A nested schema conversion failed.
    #[error("Failed to convert nested schema: {0}")]
    NestedConversion(#[source] Box<Error>),

    /// Multiple types in a single schema are not supported.
    #[error("Multiple types not supported")]
    MultipleTypesNotSupported,

    /// Custom string formats are not supported.
    #[error("Custom format not supported: {0}")]
    CustomFormatNotSupported(String),
}

impl TryFrom<utoipa::openapi::Schema> for Schema {
    type Error = Error;

    fn try_from(value: utoipa::openapi::Schema) -> Result<Self, Self::Error> {
        match value {
            utoipa::openapi::Schema::Array(array) => convert_array_schema(array),
            utoipa::openapi::Schema::Object(object) => convert_object_schema(object),
            utoipa::openapi::Schema::OneOf(one_of) => convert_one_of_schema(one_of),
            utoipa::openapi::Schema::AllOf(all_of) => convert_all_of_schema(all_of),
            utoipa::openapi::Schema::AnyOf(any_of) => convert_any_of_schema(any_of),
            _ => Err(Error::UnsupportedSchemaType),
        }
    }
}

/// Converts a utoipa Array schema to an open-rpc Schema.
fn convert_array_schema(array: utoipa::openapi::schema::Array) -> Result<Schema, Error> {
    let items = match array.items {
        utoipa::openapi::schema::ArrayItems::RefOrSchema(ref_or_schema) => {
            Some(Box::new(convert_ref_or_schema(*ref_or_schema)?))
        }
        utoipa::openapi::schema::ArrayItems::False => None,
    };

    Ok(Schema {
        title: array.title,
        description: array.description,
        contents: Some(SchemaContents::Literal(Literal::Array(ArrayLiteral {
            items,
        }))),
    })
}

/// Converts a utoipa Object schema to an open-rpc Schema.
///
/// This handles both actual object types and primitive types that are represented
/// as objects in utoipa (like strings with constraints, numbers, etc.).
fn convert_object_schema(object: utoipa::openapi::schema::Object) -> Result<Schema, Error> {
    // Determine if this is a primitive type based on schema_type
    match &object.schema_type {
        SchemaType::Type(Type::Object) => {
            // This is an actual object type
            let mut properties = BTreeMap::new();
            for (key, prop_schema) in object.properties {
                properties.insert(key, convert_ref_or_schema(prop_schema)?);
            }

            Ok(Schema {
                title: object.title,
                description: object.description,
                contents: Some(SchemaContents::Literal(Literal::Object(ObjectLiteral {
                    properties,
                    required: object.required,
                }))),
            })
        }
        SchemaType::Type(Type::String) => {
            // This is a string type with constraints
            let format = object.format.map(convert_string_format).transpose()?;

            let enumeration = object.enum_values.map(|values| {
                values
                    .into_iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

            Ok(Schema {
                title: object.title,
                description: object.description,
                contents: Some(SchemaContents::Literal(Literal::String(StringLiteral {
                    min_length: object.min_length.map(|v| v as u64),
                    max_length: object.max_length.map(|v| v as u64),
                    pattern: object.pattern,
                    format,
                    enumeration,
                }))),
            })
        }
        SchemaType::Type(Type::Integer) => {
            // This is an integer type with constraints
            let multiple_of = object.multiple_of.and_then(|n| convert_number_to_i64(&n));

            let minimum = object.minimum.and_then(|n| convert_number_to_i64(&n));

            let maximum = object.maximum.and_then(|n| convert_number_to_i64(&n));

            Ok(Schema {
                title: object.title,
                description: object.description,
                contents: Some(SchemaContents::Literal(Literal::Integer(IntegerLiteral {
                    multiple_of,
                    minimum,
                    maximum,
                    exclusive_minimum: object.exclusive_minimum.is_some(),
                    exclusive_maximum: object.exclusive_maximum.is_some(),
                }))),
            })
        }
        SchemaType::Type(Type::Number) => {
            // This is a number type with constraints
            let multiple_of = object.multiple_of.and_then(|n| convert_number_to_f64(&n));
            let minimum = object.minimum.and_then(|n| convert_number_to_f64(&n));
            let maximum = object.maximum.and_then(|n| convert_number_to_f64(&n));

            Ok(Schema {
                title: object.title,
                description: object.description,
                contents: Some(SchemaContents::Literal(Literal::Number(NumberLiteral {
                    multiple_of,
                    minimum,
                    maximum,
                    exclusive_minimum: object.exclusive_minimum.is_some(),
                    exclusive_maximum: object.exclusive_maximum.is_some(),
                }))),
            })
        }
        SchemaType::Type(Type::Boolean) => Ok(Schema {
            title: object.title,
            description: object.description,
            contents: Some(SchemaContents::Literal(Literal::Boolean)),
        }),
        SchemaType::Type(Type::Null) => Ok(Schema {
            title: object.title,
            description: object.description,
            contents: Some(SchemaContents::Literal(Literal::Null)),
        }),
        SchemaType::Type(Type::Array) => {
            // This shouldn't happen as arrays should use Schema::Array variant
            Err(Error::UnsupportedSchemaType)
        }
        SchemaType::Array(_) => Err(Error::MultipleTypesNotSupported),
        SchemaType::AnyValue => {
            // For AnyValue, we'll create an object without type constraints
            let mut properties = BTreeMap::new();
            for (key, prop_schema) in object.properties {
                properties.insert(key, convert_ref_or_schema(prop_schema)?);
            }

            Ok(Schema {
                title: object.title,
                description: object.description,
                contents: Some(SchemaContents::Literal(Literal::Object(ObjectLiteral {
                    properties,
                    required: object.required,
                }))),
            })
        }
    }
}

/// Converts a utoipa OneOf schema to an open-rpc Schema.
fn convert_one_of_schema(one_of: utoipa::openapi::schema::OneOf) -> Result<Schema, Error> {
    let mut schemas = Vec::new();
    for item in one_of.items {
        schemas.push(convert_ref_or_schema(item)?);
    }

    Ok(Schema {
        title: one_of.title,
        description: one_of.description,
        contents: Some(SchemaContents::OneOf { one_of: schemas }),
    })
}

/// Converts a utoipa AllOf schema to an open-rpc Schema.
fn convert_all_of_schema(all_of: utoipa::openapi::schema::AllOf) -> Result<Schema, Error> {
    let mut schemas = Vec::new();
    for item in all_of.items {
        schemas.push(convert_ref_or_schema(item)?);
    }

    Ok(Schema {
        title: all_of.title,
        description: all_of.description,
        contents: Some(SchemaContents::AllOf { all_of: schemas }),
    })
}

/// Converts a utoipa AnyOf schema to an open-rpc Schema.
fn convert_any_of_schema(any_of: utoipa::openapi::schema::AnyOf) -> Result<Schema, Error> {
    let mut schemas = Vec::new();
    for item in any_of.items {
        schemas.push(convert_ref_or_schema(item)?);
    }

    Ok(Schema {
        title: None, // AnyOf doesn't have title in utoipa
        description: any_of.description,
        contents: Some(SchemaContents::AnyOf { any_of: schemas }),
    })
}

/// Converts a utoipa RefOr<Schema> to an open-rpc Schema.
///
/// This handles both direct schema values and schema references.
fn convert_ref_or_schema(ref_or: RefOr<utoipa::openapi::Schema>) -> Result<Schema, Error> {
    match ref_or {
        RefOr::Ref(reference) => Ok(Schema {
            title: if reference.summary.is_empty() {
                None
            } else {
                Some(reference.summary)
            },
            description: if reference.description.is_empty() {
                None
            } else {
                Some(reference.description)
            },
            contents: Some(SchemaContents::Reference {
                reference: reference.ref_location,
            }),
        }),
        RefOr::T(schema) => schema.try_into(),
    }
}

/// Converts a utoipa SchemaFormat to an open-rpc StringFormat.
fn convert_string_format(format: SchemaFormat) -> Result<StringFormat, Error> {
    match format {
        SchemaFormat::KnownFormat(known) => convert_known_format(known),
        SchemaFormat::Custom(custom) => Err(Error::CustomFormatNotSupported(custom)),
    }
}

/// Converts a utoipa KnownFormat to an open-rpc StringFormat.
///
/// Not all utoipa formats have equivalents in open-rpc, so some will
/// result in an UnsupportedFormat error.
fn convert_known_format(format: KnownFormat) -> Result<StringFormat, Error> {
    match format {
        KnownFormat::DateTime => Ok(StringFormat::DateTime),
        KnownFormat::Time => Ok(StringFormat::Time),
        KnownFormat::Date => Ok(StringFormat::Date),
        KnownFormat::Duration => Ok(StringFormat::Duration),
        KnownFormat::Email => Ok(StringFormat::Email),
        KnownFormat::IdnEmail => Ok(StringFormat::IdnEmail),
        KnownFormat::Hostname => Ok(StringFormat::Hostname),
        KnownFormat::IdnHostname => Ok(StringFormat::IdnHostname),
        KnownFormat::Ipv4 => Ok(StringFormat::IpV4),
        KnownFormat::Ipv6 => Ok(StringFormat::IpV6),
        KnownFormat::UriTemplate => Ok(StringFormat::UriTemplate),
        KnownFormat::JsonPointer => Ok(StringFormat::JsonPointer),
        KnownFormat::RelativeJsonPointer => Ok(StringFormat::RelativeJsonPointer),
        KnownFormat::Regex => Ok(StringFormat::Regex),
        // All other formats don't have direct equivalents in open-rpc StringFormat
        // or are conditionally compiled in utoipa
        _ => Err(Error::UnsupportedFormat),
    }
}

/// Converts a utoipa Number to an i64, if possible.
fn convert_number_to_i64(number: &utoipa::Number) -> Option<i64> {
    match number {
        utoipa::Number::Int(i) => Some(*i as i64),
        utoipa::Number::UInt(u) => Some(*u as i64),
        utoipa::Number::Float(f) => Some(*f as i64),
    }
}

/// Converts a utoipa Number to an f64.
fn convert_number_to_f64(number: &utoipa::Number) -> Option<f64> {
    match number {
        utoipa::Number::Int(i) => Some(*i as f64),
        utoipa::Number::UInt(u) => Some(*u as f64),
        utoipa::Number::Float(f) => Some(*f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::openapi::schema::{ArrayBuilder, ObjectBuilder, OneOfBuilder, Type};

    #[test]
    fn test_convert_string_schema() {
        let utoipa_schema = utoipa::openapi::Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .title(Some("Test String"))
                .description(Some("A test string"))
                .min_length(Some(1))
                .max_length(Some(100))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.title, Some("Test String".to_string()));
        assert_eq!(result.description, Some("A test string".to_string()));

        if let Some(SchemaContents::Literal(Literal::String(string_literal))) = result.contents {
            assert_eq!(string_literal.min_length, Some(1));
            assert_eq!(string_literal.max_length, Some(100));
        } else {
            panic!("Expected string literal");
        }
    }

    #[test]
    fn test_convert_object_schema() {
        let utoipa_schema = utoipa::openapi::Schema::Object(
            ObjectBuilder::new()
                .property(
                    "name",
                    ObjectBuilder::new().schema_type(Type::String).build(),
                )
                .required("name")
                .title(Some("Test Object"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.title, Some("Test Object".to_string()));

        if let Some(SchemaContents::Literal(Literal::Object(object_literal))) = result.contents {
            assert!(object_literal.properties.contains_key("name"));
            assert!(object_literal.required.contains(&"name".to_string()));
        } else {
            panic!("Expected object literal");
        }
    }

    #[test]
    fn test_convert_array_schema() {
        let utoipa_schema = utoipa::openapi::Schema::Array(
            ArrayBuilder::new()
                .items(ObjectBuilder::new().schema_type(Type::String).build())
                .title(Some("Test Array"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.title, Some("Test Array".to_string()));

        if let Some(SchemaContents::Literal(Literal::Array(array_literal))) = result.contents {
            assert!(array_literal.items.is_some());
        } else {
            panic!("Expected array literal");
        }
    }

    #[test]
    fn test_convert_one_of_schema() {
        let utoipa_schema = utoipa::openapi::Schema::OneOf(
            OneOfBuilder::new()
                .item(ObjectBuilder::new().schema_type(Type::String).build())
                .item(ObjectBuilder::new().schema_type(Type::Integer).build())
                .title(Some("Test OneOf"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.title, Some("Test OneOf".to_string()));

        if let Some(SchemaContents::OneOf { one_of }) = result.contents {
            assert_eq!(one_of.len(), 2);
        } else {
            panic!("Expected OneOf schema");
        }
    }

    #[test]
    fn test_convert_integer_schema() {
        let utoipa_schema = utoipa::openapi::Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::Integer)
                .minimum(Some(utoipa::Number::Int(1)))
                .maximum(Some(utoipa::Number::Int(100)))
                .title(Some("Test Integer"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.title, Some("Test Integer".to_string()));

        if let Some(SchemaContents::Literal(Literal::Integer(int_literal))) = result.contents {
            assert_eq!(int_literal.minimum, Some(1));
            assert_eq!(int_literal.maximum, Some(100));
        } else {
            panic!("Expected integer literal");
        }
    }

    #[test]
    fn test_convert_number_schema() {
        let utoipa_schema = utoipa::openapi::Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::Number)
                .minimum(Some(utoipa::Number::Float(1.5)))
                .maximum(Some(utoipa::Number::Float(99.9)))
                .title(Some("Test Number"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.title, Some("Test Number".to_string()));

        if let Some(SchemaContents::Literal(Literal::Number(num_literal))) = result.contents {
            assert_eq!(num_literal.minimum, Some(1.5));
            assert_eq!(num_literal.maximum, Some(99.9));
        } else {
            panic!("Expected number literal");
        }
    }

    #[test]
    fn test_convert_boolean_schema() {
        let utoipa_schema = utoipa::openapi::Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::Boolean)
                .title(Some("Test Boolean"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.title, Some("Test Boolean".to_string()));

        if let Some(SchemaContents::Literal(Literal::Boolean)) = result.contents {
            // Success
        } else {
            panic!("Expected boolean literal");
        }
    }

    #[test]
    fn test_convert_all_of_schema() {
        use utoipa::openapi::schema::AllOfBuilder;

        let utoipa_schema = utoipa::openapi::Schema::AllOf(
            AllOfBuilder::new()
                .item(ObjectBuilder::new().schema_type(Type::String).build())
                .item(ObjectBuilder::new().schema_type(Type::Integer).build())
                .title(Some("Test AllOf"))
                .description(Some("A test AllOf schema"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.title, Some("Test AllOf".to_string()));
        assert_eq!(result.description, Some("A test AllOf schema".to_string()));

        if let Some(SchemaContents::AllOf { all_of }) = result.contents {
            assert_eq!(all_of.len(), 2);
        } else {
            panic!("Expected AllOf schema");
        }
    }

    #[test]
    fn test_convert_any_of_schema() {
        use utoipa::openapi::schema::AnyOfBuilder;

        let utoipa_schema = utoipa::openapi::Schema::AnyOf(
            AnyOfBuilder::new()
                .item(ObjectBuilder::new().schema_type(Type::String).build())
                .item(ObjectBuilder::new().schema_type(Type::Boolean).build())
                .description(Some("A test AnyOf schema"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();
        assert_eq!(result.description, Some("A test AnyOf schema".to_string()));

        if let Some(SchemaContents::AnyOf { any_of }) = result.contents {
            assert_eq!(any_of.len(), 2);
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_convert_reference_schema() {
        use utoipa::openapi::{Ref, RefOr};

        let reference = Ref::new("#/components/schemas/User".to_string());
        let ref_or_schema = RefOr::Ref(reference);

        // Convert through a schema that contains a reference
        let result = convert_ref_or_schema(ref_or_schema).unwrap();

        if let Some(SchemaContents::Reference { reference }) = result.contents {
            assert_eq!(reference, "#/components/schemas/User");
        } else {
            panic!("Expected reference schema");
        }
    }

    #[test]
    fn test_convert_string_with_enum() {
        let enum_values = vec![
            serde_json::Value::String("option1".to_string()),
            serde_json::Value::String("option2".to_string()),
            serde_json::Value::String("option3".to_string()),
        ];

        let utoipa_schema = utoipa::openapi::Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(enum_values))
                .title(Some("Test Enum String"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema).unwrap();

        if let Some(SchemaContents::Literal(Literal::String(string_literal))) = result.contents {
            assert!(string_literal.enumeration.is_some());
            let enum_vals = string_literal.enumeration.unwrap();
            assert_eq!(enum_vals.len(), 3);
            assert!(enum_vals.contains(&"option1".to_string()));
        } else {
            panic!("Expected string literal with enum");
        }
    }

    #[test]
    fn test_convert_unsupported_format() {
        use utoipa::openapi::schema::{KnownFormat, SchemaFormat};

        let format = SchemaFormat::KnownFormat(KnownFormat::Binary);
        let result = convert_string_format(format);

        assert!(result.is_err());
        if let Err(Error::UnsupportedFormat) = result {
            // Expected error
        } else {
            panic!("Expected UnsupportedFormat error");
        }
    }

    #[test]
    fn test_convert_custom_format() {
        use utoipa::openapi::schema::SchemaFormat;

        let format = SchemaFormat::Custom("custom-format".to_string());
        let result = convert_string_format(format);

        assert!(result.is_err());
        if let Err(Error::CustomFormatNotSupported(custom)) = result {
            assert_eq!(custom, "custom-format");
        } else {
            panic!("Expected CustomFormatNotSupported error");
        }
    }

    #[test]
    fn test_convert_multiple_types_not_supported() {
        use utoipa::openapi::schema::{SchemaType, Type};

        let utoipa_schema = utoipa::openapi::Schema::Object(
            ObjectBuilder::new()
                .schema_type(SchemaType::Array(vec![Type::String, Type::Integer]))
                .title(Some("Multiple Types"))
                .build(),
        );

        let result = Schema::try_from(utoipa_schema);

        assert!(result.is_err());
        if let Err(Error::MultipleTypesNotSupported) = result {
            // Expected error
        } else {
            panic!("Expected MultipleTypesNotSupported error");
        }
    }
}
