use axum::http::Method;
#[cfg(test)]
use bytes::Bytes;
use serde_json::{Map, Value};

const RESPONSES_CREATE_PATH: &str = "/v1/responses";
const CHAT_COMPLETIONS_PATH: &str = "/v1/chat/completions";
const DEFAULT_JSON_SCHEMA_NAME: &str = "response";

/// 快速检查字节切片中是否包含指定的子串
#[cfg(test)]
fn byte_slice_contains(haystack: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(haystack, needle).is_some()
}

#[cfg(test)]
pub(crate) fn request_body_may_need_normalization(
    method: &Method,
    path: &str,
    body: &Bytes,
) -> bool {
    if method != Method::POST || body.is_empty() {
        return false;
    }

    match path {
        CHAT_COMPLETIONS_PATH => byte_slice_contains(body, br#""response_format""#),
        RESPONSES_CREATE_PATH => {
            // 性能优化：使用 aho-corasick 多模式匹配，一次遍历检查两个模式
            // 相比两次独立的字节搜索，避免重复遍历请求体
            static RESPONSE_FORMAT_MATCHER: std::sync::OnceLock<aho_corasick::AhoCorasick> =
                std::sync::OnceLock::new();
            let matcher = RESPONSE_FORMAT_MATCHER.get_or_init(|| {
                aho_corasick::AhoCorasick::new([r#""response_format""#, r#""format""#])
                    .expect("failed to build response format matcher")
            });
            matcher.is_match(body)
        }
        _ => false,
    }
}

#[cfg(test)]
pub(crate) fn normalize_request_body(method: &Method, path: &str, body: &Bytes) -> Option<Bytes> {
    if !request_body_may_need_normalization(method, path, body) {
        return None;
    }

    let mut value: Value = serde_json::from_slice(body).ok()?;
    normalize_request_value(method, path, &mut value)
        .then(|| serde_json::to_vec(&value).ok().map(Bytes::from))
        .flatten()
}

pub(crate) fn normalize_request_value(method: &Method, path: &str, value: &mut Value) -> bool {
    if method != Method::POST || !matches!(path, RESPONSES_CREATE_PATH | CHAT_COMPLETIONS_PATH) {
        return false;
    }

    match path {
        CHAT_COMPLETIONS_PATH => normalize_chat_completions_request(value),
        RESPONSES_CREATE_PATH => normalize_responses_request(value),
        _ => false,
    }
}

pub(crate) fn normalize_json_schema_format(format: &mut Value) -> bool {
    let Some(object) = format.as_object_mut() else {
        return false;
    };
    if object.get("type").and_then(Value::as_str) != Some("json_schema") {
        return false;
    }

    if let Some(json_schema) = object.get_mut("json_schema") {
        return normalize_named_json_schema(json_schema);
    }

    normalize_named_json_schema(format)
}

fn normalize_chat_completions_request(value: &mut Value) -> bool {
    value
        .get_mut("response_format")
        .map(normalize_json_schema_format)
        .unwrap_or(false)
}

fn normalize_responses_request(value: &mut Value) -> bool {
    let response_format_changed = value
        .get_mut("response_format")
        .map(normalize_json_schema_format)
        .unwrap_or(false);
    let text_format_changed = value
        .get_mut("text")
        .and_then(|text| text.get_mut("format"))
        .map(normalize_json_schema_format)
        .unwrap_or(false);

    response_format_changed || text_format_changed
}

fn normalize_named_json_schema(json_schema: &mut Value) -> bool {
    let Some(object) = json_schema.as_object_mut() else {
        return false;
    };

    let mut changed = false;
    if !object
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(valid_json_schema_name)
    {
        object.insert(
            "name".to_owned(),
            Value::String(DEFAULT_JSON_SCHEMA_NAME.to_owned()),
        );
        changed = true;
    }

    changed |= object
        .get_mut("schema")
        .map(normalize_schema_value)
        .unwrap_or(false);

    changed
}

fn valid_json_schema_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn normalize_schema_value(schema: &mut Value) -> bool {
    match schema {
        Value::Object(object) => normalize_schema_object(object),
        Value::Array(items) => normalize_schema_values(items.iter_mut()),
        _ => false,
    }
}

fn normalize_schema_object(object: &mut Map<String, Value>) -> bool {
    let mut changed = false;

    if is_object_schema(object) && object.get("additionalProperties") != Some(&Value::Bool(false)) {
        object.insert("additionalProperties".to_owned(), Value::Bool(false));
        changed = true;
    }

    changed |= normalize_schema_map_values(object, "properties");
    changed |= normalize_schema_map_values(object, "patternProperties");
    changed |= normalize_schema_map_values(object, "$defs");
    changed |= normalize_schema_map_values(object, "definitions");
    changed |= normalize_schema_map_values(object, "dependentSchemas");

    changed |= normalize_schema_child(object, "items");
    changed |= normalize_schema_child(object, "contains");
    changed |= normalize_schema_child(object, "not");
    changed |= normalize_schema_child(object, "if");
    changed |= normalize_schema_child(object, "then");
    changed |= normalize_schema_child(object, "else");
    changed |= normalize_schema_child(object, "propertyNames");
    changed |= normalize_schema_child(object, "additionalItems");
    changed |= normalize_schema_child(object, "unevaluatedItems");
    changed |= normalize_schema_child(object, "unevaluatedProperties");

    changed |= normalize_schema_array_items(object, "prefixItems");
    changed |= normalize_schema_array_items(object, "allOf");
    changed |= normalize_schema_array_items(object, "anyOf");
    changed |= normalize_schema_array_items(object, "oneOf");

    changed
}

fn is_object_schema(object: &Map<String, Value>) -> bool {
    object.contains_key("properties")
        || object.contains_key("additionalProperties")
        || match object.get("type") {
            Some(Value::String(schema_type)) => schema_type == "object",
            Some(Value::Array(schema_types)) => schema_types
                .iter()
                .any(|schema_type| schema_type.as_str() == Some("object")),
            _ => false,
        }
}

fn normalize_schema_map_values(object: &mut Map<String, Value>, key: &str) -> bool {
    object
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .map(|schemas| normalize_schema_values(schemas.values_mut()))
        .unwrap_or(false)
}

fn normalize_schema_child(object: &mut Map<String, Value>, key: &str) -> bool {
    object
        .get_mut(key)
        .map(normalize_schema_value)
        .unwrap_or(false)
}

fn normalize_schema_array_items(object: &mut Map<String, Value>, key: &str) -> bool {
    object
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .map(|schemas| normalize_schema_values(schemas.iter_mut()))
        .unwrap_or(false)
}

fn normalize_schema_values<'a>(schemas: impl Iterator<Item = &'a mut Value>) -> bool {
    let mut changed = false;
    for schema in schemas {
        changed |= normalize_schema_value(schema);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_chat_response_format_nested_object_items() {
        let body = Bytes::from(
            json!({
                "model": "gpt-5.5",
                "messages": [{"role": "user", "content": "analyze"}],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "DeepAnalysisLLMOut",
                        "strict": true,
                        "schema": {
                            "type": "object",
                            "properties": {
                                "affected_entities": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "name": {"type": "string"}
                                        },
                                        "required": ["name"]
                                    }
                                }
                            },
                            "required": ["affected_entities"]
                        }
                    }
                }
            })
            .to_string(),
        );

        let normalized =
            normalize_request_body(&Method::POST, CHAT_COMPLETIONS_PATH, &body).unwrap();
        let value: Value = serde_json::from_slice(&normalized).unwrap();

        assert_eq!(
            value["response_format"]["json_schema"]["schema"]["additionalProperties"],
            false
        );
        assert_eq!(
            value["response_format"]["json_schema"]["schema"]["properties"]["affected_entities"]["items"]
                ["additionalProperties"],
            false
        );
    }

    #[test]
    fn normalizes_responses_text_format_nested_object_items() {
        let body = Bytes::from(
            json!({
                "model": "gpt-5.5",
                "input": "analyze",
                "text": {
                    "format": {
                        "type": "json_schema",
                        "name": "DeepAnalysisLLMOut",
                        "strict": true,
                        "schema": {
                            "type": "object",
                            "properties": {
                                "affected_entities": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "name": {"type": "string"}
                                        },
                                        "required": ["name"]
                                    }
                                }
                            },
                            "required": ["affected_entities"]
                        }
                    }
                }
            })
            .to_string(),
        );

        let normalized =
            normalize_request_body(&Method::POST, RESPONSES_CREATE_PATH, &body).unwrap();
        let value: Value = serde_json::from_slice(&normalized).unwrap();

        assert_eq!(
            value["text"]["format"]["schema"]["additionalProperties"],
            false
        );
        assert_eq!(
            value["text"]["format"]["schema"]["properties"]["affected_entities"]["items"]["additionalProperties"],
            false
        );
    }

    #[test]
    fn leaves_non_json_schema_response_format_unchanged() {
        let body = Bytes::from_static(
            br#"{"model":"demo","messages":[],"response_format":{"type":"json_object"}}"#,
        );

        assert!(normalize_request_body(&Method::POST, CHAT_COMPLETIONS_PATH, &body).is_none());
    }

    #[test]
    fn replaces_existing_additional_properties_schema_with_false() {
        let mut format = json!({
            "type": "json_schema",
            "json_schema": {
                "name": "Result",
                "schema": {
                    "type": "object",
                    "additionalProperties": {"type": "string"}
                }
            }
        });

        assert!(normalize_json_schema_format(&mut format));

        assert_eq!(
            format["json_schema"]["schema"]["additionalProperties"],
            false
        );
    }

    #[test]
    fn adds_default_name_to_json_schema_format_when_missing() {
        let mut format = json!({
            "type": "json_schema",
            "schema": {
                "type": "object",
                "properties": {
                    "answer": {"type": "string"}
                },
                "required": ["answer"]
            }
        });

        assert!(normalize_json_schema_format(&mut format));

        assert_eq!(format["name"], DEFAULT_JSON_SCHEMA_NAME);
        assert_eq!(format["schema"]["additionalProperties"], false);
    }

    #[test]
    fn replaces_invalid_json_schema_name_with_default_name() {
        let mut format = json!({
            "type": "json_schema",
            "json_schema": {
                "name": "invalid name with spaces",
                "schema": {
                    "type": "object"
                }
            }
        });

        assert!(normalize_json_schema_format(&mut format));

        assert_eq!(format["json_schema"]["name"], DEFAULT_JSON_SCHEMA_NAME);
    }

    #[test]
    fn normalizes_all_sibling_properties_after_first_change() {
        let mut format = json!({
            "type": "json_schema",
            "json_schema": {
                "name": "Siblings",
                "schema": {
                    "type": "object",
                    "properties": {
                        "first": {
                            "type": "object",
                            "properties": {"name": {"type": "string"}}
                        },
                        "second": {
                            "type": "object",
                            "properties": {"name": {"type": "string"}}
                        }
                    }
                }
            }
        });

        assert!(normalize_json_schema_format(&mut format));

        assert_eq!(
            format["json_schema"]["schema"]["properties"]["first"]["additionalProperties"],
            false
        );
        assert_eq!(
            format["json_schema"]["schema"]["properties"]["second"]["additionalProperties"],
            false
        );
    }
}
