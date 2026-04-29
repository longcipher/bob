//! # Output Validation
//!
//! Validates LLM responses against a JSON Schema for structured output support.
//!
//! When a request specifies an `output_schema`, the scheduler validates the
//! final LLM response against this schema. On failure, it re-prompts the LLM
//! with the validation error, up to `max_output_retries` times.

use serde_json::Value;

/// Validate a JSON value against a JSON Schema.
///
/// Returns `Ok(())` if valid, or `Err(message)` with a human-readable
/// validation error description.
pub(crate) fn validate_output(value: &Value, schema: &Value) -> Result<(), String> {
    let compiled =
        jsonschema::Validator::new(schema).map_err(|e| format!("invalid schema: {e}"))?;
    if compiled.is_valid(value) {
        Ok(())
    } else {
        let errors: Vec<String> = compiled.iter_errors(value).map(|e| format!("{e}")).collect();
        Err(format!("output validation failed: {}", errors.join("; ")))
    }
}

/// Validate a JSON string against a JSON Schema.
///
/// Attempts to parse the string as JSON first, then validates against the schema.
pub(crate) fn validate_output_str(content: &str, schema: &Value) -> Result<Value, String> {
    let value: Value = serde_json::from_str(content).map_err(|e| format!("invalid JSON: {e}"))?;
    validate_output(&value, schema)?;
    Ok(value)
}

/// Build a re-prompt message for a validation failure.
pub(crate) fn validation_error_prompt(content: &str, error: &str) -> String {
    format!(
        "Your previous response did not match the required schema.\n\
         Validation error: {error}\n\
         Your response was: {content}\n\
         Please respond with a valid JSON object matching the required schema."
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn person_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name", "age"]
        })
    }

    #[test]
    fn valid_output_passes() {
        let value = json!({"name": "Alice", "age": 30});
        assert!(validate_output(&value, &person_schema()).is_ok());
    }

    #[test]
    fn missing_required_field_fails() {
        let value = json!({"name": "Alice"});
        let result = validate_output(&value, &person_schema());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("age"));
    }

    #[test]
    fn wrong_type_fails() {
        let value = json!({"name": "Alice", "age": "thirty"});
        assert!(validate_output(&value, &person_schema()).is_err());
    }

    #[test]
    fn validate_output_str_parses_and_validates() {
        let content = r#"{"name": "Bob", "age": 25}"#;
        let result = validate_output_str(content, &person_schema());
        assert!(result.is_ok());
    }

    #[test]
    fn validate_output_str_rejects_invalid_json() {
        let content = "not json";
        let result = validate_output_str(content, &person_schema());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn validation_error_prompt_contains_details() {
        let prompt = validation_error_prompt("bad content", "missing field 'age'");
        assert!(prompt.contains("bad content"));
        assert!(prompt.contains("missing field 'age'"));
        assert!(prompt.contains("required schema"));
    }
}
