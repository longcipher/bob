//! # OpenAI JSON Schema Transformer
//!
//! Transforms JSON Schema to be compatible with OpenAI's structured output requirements.
//!
//! OpenAI's strict structured output mode has specific requirements:
//! - `additionalProperties: false` on all objects
//! - All properties must be listed in `required`
//! - `$ref` references must be inlined
//! - Some keywords are unsupported (`$schema`, `title`, `discriminator`, etc.)
//! - `format` must be one of 9 supported string formats
//! - `oneOf` must be converted to `anyOf`
//!
//! This module provides a recursive transformer that adapts arbitrary JSON Schema
//! to OpenAI-compatible format, preserving constraint information in `description`
//! fields where the strict mode doesn't support them.

use serde_json::Value;

/// Supported string formats in OpenAI strict mode.
const SUPPORTED_FORMATS: &[&str] =
    &["date-time", "time", "date", "duration", "email", "hostname", "ipv4", "ipv6", "uuid"];

/// Keys removed from the schema in strict mode.
const REMOVED_KEYS: &[&str] = &["$schema", "title", "discriminator", "default"];

/// Keys that are incompatible with strict mode and must be moved to description.
const INCOMPATIBLE_KEYS: &[&str] = &[
    "minLength",
    "maxLength",
    "pattern",
    "patternProperties",
    "uniqueItems",
    "minItems",
    "maxItems",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minProperties",
    "maxProperties",
    "propertyNames",
    "const",
    "examples",
];

/// Transform a JSON Schema for OpenAI strict structured output.
///
/// Returns `(transformed_schema, is_strict_compatible)` where `is_strict_compatible`
/// is `false` if the schema contains features that cannot be made strictly compatible.
pub fn transform_openai_schema(schema: &Value, strict: bool) -> (Value, bool) {
    let mut schema = schema.clone();
    let compatible = transform_value(&mut schema, strict);
    (schema, compatible)
}

fn transform_value(value: &mut Value, strict: bool) -> bool {
    let Some(obj) = value.as_object_mut() else {
        if let Some(arr) = value.as_array_mut() {
            let mut compatible = true;
            for item in arr.iter_mut() {
                if !transform_value(item, strict) {
                    compatible = false;
                }
            }
            return compatible;
        }
        return true;
    };

    let mut compatible = true;

    // Remove unsupported keys.
    for key in REMOVED_KEYS {
        obj.remove(*key);
    }

    // Convert oneOf to anyOf in strict mode.
    if strict && let Some(one_of) = obj.remove("oneOf") {
        obj.insert("anyOf".into(), one_of);
    }

    // Handle format: restrict to supported values.
    if let Some(format) = obj.get("format").and_then(|v| v.as_str()).map(String::from) &&
        strict &&
        !SUPPORTED_FORMATS.contains(&format.as_str())
    {
        let desc = obj.entry("description").or_insert_with(|| Value::String(String::new()));
        if let Some(desc_str) = desc.as_str() {
            *desc = Value::String(format!("{desc_str} [format: {format}]").trim().to_string());
        }
        obj.remove("format");
        compatible = false;
    }

    // Handle object types.
    if obj.get("type").and_then(|v| v.as_str()) == Some("object") && strict {
        obj.insert("additionalProperties".into(), Value::Bool(false));

        // Ensure all properties are required.
        if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
            let all_required: Vec<Value> = props.keys().map(|k| Value::String(k.clone())).collect();
            obj.insert("required".into(), Value::Array(all_required));
        }
    }

    // Move incompatible keys to description (all types, not just objects).
    if strict {
        let mut notes = Vec::new();
        let keys: Vec<String> = obj.keys().cloned().collect();
        for key in keys {
            if INCOMPATIBLE_KEYS.contains(&key.as_str()) &&
                let Some(val) = obj.remove(&key)
            {
                notes.push(format!("{key}: {val}"));
            }
        }
        if !notes.is_empty() {
            let desc = obj.entry("description").or_insert_with(|| Value::String(String::new()));
            if let Some(desc_str) = desc.as_str() {
                let note_str = format!("[Constraints: {}]", notes.join(", "));
                *desc = if desc_str.is_empty() {
                    Value::String(note_str)
                } else {
                    Value::String(format!("{desc_str} {note_str}"))
                };
            }
        }
    }

    // Inline $ref references.
    if obj.contains_key("$ref") {
        compatible = false;
    }

    // Recurse into properties.
    if let Some(props) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
        for prop in props.values_mut() {
            if !transform_value(prop, strict) {
                compatible = false;
            }
        }
    }

    // Recurse into items.
    if let Some(items) = obj.get_mut("items") &&
        !transform_value(items, strict)
    {
        compatible = false;
    }

    // Recurse into anyOf / allOf.
    for key in &["anyOf", "allOf"] {
        if let Some(arr) = obj.get_mut(*key).and_then(|v| v.as_array_mut()) {
            for item in arr.iter_mut() {
                if !transform_value(item, strict) {
                    compatible = false;
                }
            }
        }
    }

    compatible
}

/// Check if a schema is compatible with OpenAI strict mode.
#[must_use]
pub fn is_strict_compatible(schema: &Value) -> bool {
    let (_, compatible) = transform_openai_schema(schema, false);
    compatible
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn removes_title_and_schema() {
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "Person",
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        });
        let (transformed, _) = transform_openai_schema(&schema, true);
        assert!(!transformed.as_object().unwrap().contains_key("$schema"));
        assert!(!transformed.as_object().unwrap().contains_key("title"));
    }

    #[test]
    fn adds_additional_properties_false() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        });
        let (transformed, _) = transform_openai_schema(&schema, true);
        assert_eq!(transformed["additionalProperties"], false);
    }

    #[test]
    fn makes_all_properties_required() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name"]
        });
        let (transformed, _) = transform_openai_schema(&schema, true);
        let required = transformed["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.iter().any(|v| v == "name"));
        assert!(required.iter().any(|v| v == "age"));
    }

    #[test]
    fn converts_oneof_to_anyof() {
        let schema = json!({
            "oneOf": [
                {"type": "string"},
                {"type": "integer"}
            ]
        });
        let (transformed, _) = transform_openai_schema(&schema, true);
        assert!(transformed.as_object().unwrap().contains_key("anyOf"));
        assert!(!transformed.as_object().unwrap().contains_key("oneOf"));
    }

    #[test]
    fn moves_incompatible_constraints_to_description() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 100
                }
            }
        });
        let (transformed, _) = transform_openai_schema(&schema, true);
        let name_schema = &transformed["properties"]["name"];
        assert!(!name_schema.as_object().unwrap().contains_key("minLength"));
        assert!(!name_schema.as_object().unwrap().contains_key("maxLength"));
        let desc = name_schema["description"].as_str().unwrap();
        assert!(desc.contains("minLength"));
        assert!(desc.contains("maxLength"));
    }

    #[test]
    fn unsupported_format_moved_to_description() {
        let schema = json!({
            "type": "object",
            "properties": {
                "color": {"type": "string", "format": "color"}
            }
        });
        let (transformed, compatible) = transform_openai_schema(&schema, true);
        let color_schema = &transformed["properties"]["color"];
        assert!(!color_schema.as_object().unwrap().contains_key("format"));
        assert!(!compatible);
        let desc = color_schema["description"].as_str().unwrap();
        assert!(desc.contains("color"));
    }

    #[test]
    fn supported_format_preserved() {
        let schema = json!({
            "type": "object",
            "properties": {
                "email": {"type": "string", "format": "email"}
            }
        });
        let (transformed, compatible) = transform_openai_schema(&schema, true);
        assert_eq!(transformed["properties"]["email"]["format"], "email");
        assert!(compatible);
    }

    #[test]
    fn nested_objects_transformed() {
        let schema = json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "object",
                    "properties": {
                        "street": {"type": "string"},
                        "city": {"type": "string"}
                    }
                }
            }
        });
        let (transformed, _) = transform_openai_schema(&schema, true);
        let addr = &transformed["properties"]["address"];
        assert_eq!(addr["additionalProperties"], false);
        assert_eq!(addr["required"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn array_items_transformed() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"}
                        }
                    }
                }
            }
        });
        let (transformed, _) = transform_openai_schema(&schema, true);
        let items = &transformed["properties"]["tags"]["items"];
        assert_eq!(items["additionalProperties"], false);
    }
}
