use std::str::FromStr;

use anyhow::{Context, Result, bail};
use axum::http::{Method, Uri};
use bytes::Bytes;
use serde_json::{Map, Value, json};

use crate::structured_outputs;

const RESPONSES_CREATE_PATH: &str = "/v1/responses";
const CHAT_COMPLETIONS_PATH: &str = "/v1/chat/completions";

pub(crate) fn is_responses_create_endpoint(method: &Method, path: &str) -> bool {
    method.as_str() == "POST" && path == RESPONSES_CREATE_PATH
}

pub(crate) fn uses_image_generation_tool(body: &Bytes) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("tools")
                .and_then(Value::as_array)
                .map(|tools| tools.iter().any(tool_is_image_generation))
        })
        .unwrap_or(false)
}

fn tool_is_image_generation(tool: &Value) -> bool {
    tool.get("type")
        .and_then(Value::as_str)
        .is_some_and(|tool_type| tool_type == "image_generation")
}

pub(crate) fn chat_completions_uri_from_responses_uri(uri: &Uri) -> Uri {
    let query = uri
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    Uri::from_str(&format!("{CHAT_COMPLETIONS_PATH}{query}"))
        .expect("static chat completions path and existing query should form a valid URI")
}

pub(crate) fn responses_to_chat_body(body: &Bytes) -> Result<Bytes> {
    let value: Value = serde_json::from_slice(body)
        .context("Responses fallback request body must be valid JSON")?;
    let source = value
        .as_object()
        .context("Responses fallback request body must be a JSON object")?;

    let mut chat = Map::new();
    copy_keys(
        source,
        &mut chat,
        &[
            "model",
            "temperature",
            "top_p",
            "presence_penalty",
            "frequency_penalty",
            "stop",
            "user",
            "metadata",
            "store",
            "seed",
            "logprobs",
            "top_logprobs",
            "n",
            "service_tier",
            "parallel_tool_calls",
        ],
    );

    if let Some(value) = source.get("max_output_tokens") {
        chat.insert("max_tokens".to_owned(), value.clone());
    } else if let Some(value) = source.get("max_tokens") {
        chat.insert("max_tokens".to_owned(), value.clone());
    }
    copy_keys(source, &mut chat, &["max_completion_tokens"]);

    let messages = chat_messages_from_responses(source)?;
    chat.insert("messages".to_owned(), Value::Array(messages));

    if let Some(tools) = source.get("tools") {
        chat.insert("tools".to_owned(), convert_tools(tools)?);
    }
    if let Some(tool_choice) = source.get("tool_choice") {
        chat.insert("tool_choice".to_owned(), convert_tool_choice(tool_choice));
    }

    if let Some(response_format) = source.get("response_format") {
        let mut response_format = response_format.clone();
        structured_outputs::normalize_json_schema_format(&mut response_format);
        chat.insert("response_format".to_owned(), response_format);
    } else if let Some(response_format) = source
        .get("text")
        .and_then(|text| text.get("format"))
        .and_then(convert_text_format)
    {
        chat.insert("response_format".to_owned(), response_format);
    }

    chat.insert("stream".to_owned(), Value::Bool(false));

    serde_json::to_vec(&Value::Object(chat))
        .map(Bytes::from)
        .context("failed to serialize Chat Completions fallback request")
}

pub(crate) fn chat_to_responses_body(body: &Bytes) -> Result<Bytes> {
    let value: Value = serde_json::from_slice(body)
        .context("Chat Completions fallback response body must be valid JSON")?;

    if value.get("object").and_then(Value::as_str) == Some("response") {
        return Ok(body.clone());
    }

    let source = value
        .as_object()
        .context("Chat Completions fallback response body must be a JSON object")?;

    let mut response = Map::new();
    response.insert(
        "id".to_owned(),
        source
            .get("id")
            .cloned()
            .unwrap_or_else(|| Value::String("resp_chat_fallback".to_owned())),
    );
    response.insert("object".to_owned(), Value::String("response".to_owned()));
    if let Some(created) = source.get("created") {
        response.insert("created_at".to_owned(), created.clone());
    }
    copy_keys(source, &mut response, &["model", "system_fingerprint"]);

    let mut output = Vec::new();
    let mut output_texts = Vec::new();
    let mut incomplete_reason = None;

    if let Some(choices) = source.get("choices").and_then(Value::as_array) {
        for (choice_position, choice) in choices.iter().enumerate() {
            let choice_index = choice
                .get("index")
                .and_then(Value::as_i64)
                .unwrap_or(choice_position as i64);

            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str)
                && (reason == "length" || reason == "content_filter")
            {
                incomplete_reason = Some(reason.to_owned());
            }

            let Some(message) = choice.get("message").and_then(Value::as_object) else {
                continue;
            };

            if let Some(message_output) =
                response_message_from_chat_message(message, choice_index, &mut output_texts)
            {
                output.push(message_output);
            }

            if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                for (tool_position, tool_call) in tool_calls.iter().enumerate() {
                    if let Some(function_call) = response_function_call_from_chat_tool_call(
                        tool_call,
                        choice_index,
                        tool_position,
                    ) {
                        output.push(function_call);
                    }
                }
            }

            if let Some(function_call) = message.get("function_call")
                && let Some(function_call) = response_function_call_from_legacy_chat_function_call(
                    function_call,
                    choice_index,
                )
            {
                output.push(function_call);
            }
        }
    }

    response.insert("output".to_owned(), Value::Array(output));
    response.insert(
        "output_text".to_owned(),
        Value::String(output_texts.join("")),
    );

    if let Some(usage) = source.get("usage") {
        response.insert("usage".to_owned(), convert_usage(usage));
    }

    if let Some(reason) = incomplete_reason {
        response.insert("status".to_owned(), Value::String("incomplete".to_owned()));
        response.insert(
            "incomplete_details".to_owned(),
            json!({
                "reason": match reason.as_str() {
                    "length" => "max_output_tokens",
                    other => other,
                }
            }),
        );
    } else {
        response.insert("status".to_owned(), Value::String("completed".to_owned()));
    }

    serde_json::to_vec(&Value::Object(response))
        .map(Bytes::from)
        .context("failed to serialize Responses fallback response")
}

fn copy_keys(source: &Map<String, Value>, target: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(value) = source.get(*key) {
            target.insert((*key).to_owned(), value.clone());
        }
    }
}

fn chat_messages_from_responses(source: &Map<String, Value>) -> Result<Vec<Value>> {
    let mut messages = Vec::new();

    if let Some(instructions) = source.get("instructions").and_then(value_to_optional_text)
        && !instructions.is_empty()
    {
        messages.push(json!({
            "role": "system",
            "content": instructions,
        }));
    }

    if let Some(input) = source.get("input") {
        messages.extend(messages_from_response_input(input)?);
    } else if let Some(existing_messages) = source.get("messages") {
        messages.extend(messages_from_response_input(existing_messages)?);
    } else if let Some(prompt) = source.get("prompt").and_then(value_to_optional_text) {
        messages.push(json!({
            "role": "user",
            "content": prompt,
        }));
    }

    if messages.is_empty() {
        bail!("Responses fallback request must contain input, messages, or prompt text");
    }

    Ok(messages)
}

fn messages_from_response_input(input: &Value) -> Result<Vec<Value>> {
    match input {
        Value::String(text) => Ok(vec![json!({
            "role": "user",
            "content": text,
        })]),
        Value::Array(items) => {
            let mut messages = Vec::new();
            for item in items {
                if let Some(message) = message_from_response_input_item(item)? {
                    messages.push(message);
                }
            }
            Ok(messages)
        }
        Value::Object(_) => Ok(message_from_response_input_item(input)?
            .into_iter()
            .collect()),
        _ => bail!("Responses fallback input must be a string, object, or array"),
    }
}

fn message_from_response_input_item(item: &Value) -> Result<Option<Value>> {
    match item {
        Value::String(text) => Ok(Some(json!({
            "role": "user",
            "content": text,
        }))),
        Value::Object(object) => {
            let item_type = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match item_type {
                "function_call_output" => Ok(Some(tool_message_from_response_output(object))),
                "function_call" => Ok(Some(assistant_message_from_response_function_call(object))),
                "reasoning" | "item_reference" => Ok(None),
                "message" => message_from_response_message_object(object).map(Some),
                _ if object.contains_key("role") || object.contains_key("content") => {
                    message_from_response_message_object(object).map(Some)
                }
                _ if object.contains_key("text") => Ok(Some(json!({
                    "role": "user",
                    "content": value_to_optional_text(&object["text"]).unwrap_or_default(),
                }))),
                _ => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

fn message_from_response_message_object(object: &Map<String, Value>) -> Result<Value> {
    let role = object
        .get("role")
        .and_then(Value::as_str)
        .map(normalize_chat_role)
        .unwrap_or("user");

    let mut message = Map::new();
    message.insert("role".to_owned(), Value::String(role.to_owned()));

    if let Some(name) = object.get("name") {
        message.insert("name".to_owned(), name.clone());
    }
    if let Some(tool_call_id) = object.get("tool_call_id").or_else(|| object.get("call_id")) {
        message.insert("tool_call_id".to_owned(), tool_call_id.clone());
    }
    if let Some(tool_calls) = object.get("tool_calls") {
        message.insert("tool_calls".to_owned(), tool_calls.clone());
    }

    let content = object
        .get("content")
        .map(chat_content_from_responses_content)
        .transpose()?
        .unwrap_or_else(|| {
            if message.contains_key("tool_calls") && role == "assistant" {
                Value::Null
            } else {
                Value::String(String::new())
            }
        });

    message.insert("content".to_owned(), content);
    Ok(Value::Object(message))
}

fn normalize_chat_role(role: &str) -> &'static str {
    match role {
        "system" => "system",
        "developer" => "developer",
        "user" => "user",
        "assistant" => "assistant",
        "tool" => "tool",
        _ => "user",
    }
}

fn tool_message_from_response_output(object: &Map<String, Value>) -> Value {
    let content = object
        .get("output")
        .or_else(|| object.get("content"))
        .and_then(value_to_optional_text)
        .unwrap_or_default();
    let tool_call_id = object
        .get("call_id")
        .or_else(|| object.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("call_unknown");

    json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content,
    })
}

fn assistant_message_from_response_function_call(object: &Map<String, Value>) -> Value {
    let call_id = object
        .get("call_id")
        .or_else(|| object.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("call_unknown");
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = object
        .get("arguments")
        .map(json_value_to_argument_string)
        .unwrap_or_default();

    json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{
            "id": call_id,
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments,
            }
        }]
    })
}

fn chat_content_from_responses_content(content: &Value) -> Result<Value> {
    match content {
        Value::String(_) | Value::Null => Ok(content.clone()),
        Value::Array(parts) => {
            let mut converted = Vec::new();
            for part in parts {
                if let Some(converted_part) = chat_content_part_from_response_part(part) {
                    converted.push(converted_part);
                }
            }
            Ok(Value::Array(converted))
        }
        Value::Object(_) => Ok(chat_content_part_from_response_part(content)
            .unwrap_or_else(|| Value::String(value_to_optional_text(content).unwrap_or_default()))),
        _ => Ok(Value::String(
            value_to_optional_text(content).unwrap_or_default(),
        )),
    }
}

fn chat_content_part_from_response_part(part: &Value) -> Option<Value> {
    match part {
        Value::String(text) => Some(json!({
            "type": "text",
            "text": text,
        })),
        Value::Object(object) => {
            let part_type = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match part_type {
                "input_text" | "output_text" | "text" => object
                    .get("text")
                    .and_then(value_to_optional_text)
                    .map(|text| json!({ "type": "text", "text": text })),
                "input_image" => response_image_part_to_chat_image_part(object),
                "image_url" => Some(Value::Object(object.clone())),
                "refusal" => object
                    .get("refusal")
                    .and_then(value_to_optional_text)
                    .map(|text| json!({ "type": "text", "text": text })),
                _ => object
                    .get("text")
                    .and_then(value_to_optional_text)
                    .map(|text| json!({ "type": "text", "text": text }))
                    .or_else(|| Some(Value::Object(object.clone()))),
            }
        }
        _ => None,
    }
}

fn response_image_part_to_chat_image_part(object: &Map<String, Value>) -> Option<Value> {
    if let Some(image_url) = object.get("image_url") {
        return Some(match image_url {
            Value::String(url) => json!({
                "type": "image_url",
                "image_url": { "url": url },
            }),
            Value::Object(_) => json!({
                "type": "image_url",
                "image_url": image_url,
            }),
            _ => return None,
        });
    }

    object.get("url").and_then(Value::as_str).map(|url| {
        json!({
            "type": "image_url",
            "image_url": { "url": url },
        })
    })
}

fn convert_tools(tools: &Value) -> Result<Value> {
    let Some(items) = tools.as_array() else {
        bail!("Responses fallback tools must be an array");
    };
    Ok(Value::Array(items.iter().map(convert_tool).collect()))
}

fn convert_tool(tool: &Value) -> Value {
    let Some(object) = tool.as_object() else {
        return tool.clone();
    };

    if object.get("type").and_then(Value::as_str) != Some("function")
        || object.contains_key("function")
    {
        return tool.clone();
    }

    let mut function = Map::new();
    copy_keys(
        object,
        &mut function,
        &["name", "description", "parameters", "strict"],
    );

    json!({
        "type": "function",
        "function": Value::Object(function),
    })
}

fn convert_tool_choice(tool_choice: &Value) -> Value {
    let Some(object) = tool_choice.as_object() else {
        return tool_choice.clone();
    };

    if object.get("type").and_then(Value::as_str) == Some("function")
        && !object.contains_key("function")
        && let Some(name) = object.get("name")
    {
        return json!({
            "type": "function",
            "function": { "name": name },
        });
    }

    tool_choice.clone()
}

fn convert_text_format(format: &Value) -> Option<Value> {
    let object = format.as_object()?;
    match object.get("type").and_then(Value::as_str) {
        Some("text") => None,
        Some("json_object") => Some(json!({ "type": "json_object" })),
        Some("json_schema") => {
            if object.contains_key("json_schema") {
                let mut format = format.clone();
                structured_outputs::normalize_json_schema_format(&mut format);
                return Some(format);
            }
            let mut json_schema = Map::new();
            copy_keys(
                object,
                &mut json_schema,
                &["name", "description", "schema", "strict"],
            );
            let mut response_format = json!({
                "type": "json_schema",
                "json_schema": Value::Object(json_schema),
            });
            structured_outputs::normalize_json_schema_format(&mut response_format);
            Some(response_format)
        }
        Some(_) => Some(format.clone()),
        None => None,
    }
}

fn response_message_from_chat_message(
    message: &Map<String, Value>,
    choice_index: i64,
    output_texts: &mut Vec<String>,
) -> Option<Value> {
    let mut content = Vec::new();

    if let Some(chat_content) = message.get("content") {
        append_response_content_from_chat_content(chat_content, &mut content, output_texts);
    }

    if let Some(refusal) = message.get("refusal").and_then(Value::as_str)
        && !refusal.is_empty()
    {
        content.push(json!({
            "type": "refusal",
            "refusal": refusal,
        }));
    }

    if content.is_empty() {
        return None;
    }

    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("assistant");

    Some(json!({
        "id": format!("msg_{choice_index}"),
        "type": "message",
        "status": "completed",
        "role": role,
        "content": content,
    }))
}

fn append_response_content_from_chat_content(
    chat_content: &Value,
    output: &mut Vec<Value>,
    output_texts: &mut Vec<String>,
) {
    match chat_content {
        Value::String(text) => push_output_text(output, output_texts, text, None),
        Value::Array(parts) => {
            for part in parts {
                match part {
                    Value::String(text) => push_output_text(output, output_texts, text, None),
                    Value::Object(object) => {
                        let part_type = object
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match part_type {
                            "text" | "output_text" => {
                                let annotations = object.get("annotations").cloned();
                                if let Some(text) = object.get("text").and_then(Value::as_str) {
                                    push_output_text(output, output_texts, text, annotations);
                                }
                            }
                            "refusal" => {
                                if let Some(refusal) = object.get("refusal").and_then(Value::as_str)
                                {
                                    output.push(json!({
                                        "type": "refusal",
                                        "refusal": refusal,
                                    }));
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                push_output_text(
                    output,
                    output_texts,
                    text,
                    object.get("annotations").cloned(),
                );
            }
        }
        _ => {}
    }
}

fn push_output_text(
    output: &mut Vec<Value>,
    output_texts: &mut Vec<String>,
    text: &str,
    annotations: Option<Value>,
) {
    if text.is_empty() {
        return;
    }
    output_texts.push(text.to_owned());
    output.push(json!({
        "type": "output_text",
        "text": text,
        "annotations": annotations.unwrap_or_else(|| Value::Array(Vec::new())),
    }));
}

fn response_function_call_from_chat_tool_call(
    tool_call: &Value,
    choice_index: i64,
    tool_position: usize,
) -> Option<Value> {
    let object = tool_call.as_object()?;
    if object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function")
        != "function"
    {
        return None;
    }

    let function = object.get("function").and_then(Value::as_object)?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = function
        .get("arguments")
        .map(json_value_to_argument_string)
        .unwrap_or_default();
    let call_id = object
        .get("id")
        .or_else(|| object.get("call_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("call_{choice_index}_{tool_position}"));

    Some(json!({
        "id": format!("fc_{choice_index}_{tool_position}"),
        "type": "function_call",
        "status": "completed",
        "call_id": call_id,
        "name": name,
        "arguments": arguments,
    }))
}

fn response_function_call_from_legacy_chat_function_call(
    function_call: &Value,
    choice_index: i64,
) -> Option<Value> {
    let object = function_call.as_object()?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = object
        .get("arguments")
        .map(json_value_to_argument_string)
        .unwrap_or_default();

    Some(json!({
        "id": format!("fc_{choice_index}_0"),
        "type": "function_call",
        "status": "completed",
        "call_id": format!("call_{choice_index}_0"),
        "name": name,
        "arguments": arguments,
    }))
}

fn convert_usage(usage: &Value) -> Value {
    let Some(object) = usage.as_object() else {
        return usage.clone();
    };

    let mut converted = Map::new();
    if let Some(input_tokens) = object
        .get("input_tokens")
        .or_else(|| object.get("prompt_tokens"))
    {
        converted.insert("input_tokens".to_owned(), input_tokens.clone());
    }
    if let Some(output_tokens) = object
        .get("output_tokens")
        .or_else(|| object.get("completion_tokens"))
    {
        converted.insert("output_tokens".to_owned(), output_tokens.clone());
    }
    if let Some(total_tokens) = object.get("total_tokens") {
        converted.insert("total_tokens".to_owned(), total_tokens.clone());
    }
    if let Some(details) = object
        .get("input_tokens_details")
        .or_else(|| object.get("prompt_tokens_details"))
    {
        converted.insert("input_tokens_details".to_owned(), details.clone());
    }
    if let Some(details) = object
        .get("output_tokens_details")
        .or_else(|| object.get("completion_tokens_details"))
    {
        converted.insert("output_tokens_details".to_owned(), details.clone());
    }

    Value::Object(converted)
}

fn value_to_optional_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Null => None,
        _ => serde_json::to_string(value).ok(),
    }
}

fn json_value_to_argument_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_match_is_exact_post_responses_create_only() {
        assert!(is_responses_create_endpoint(&Method::POST, "/v1/responses"));
        assert!(!is_responses_create_endpoint(&Method::GET, "/v1/responses"));
        assert!(!is_responses_create_endpoint(
            &Method::POST,
            "/v1/responses/compact"
        ));
        assert!(!is_responses_create_endpoint(
            &Method::POST,
            "/v1/responses/resp_123"
        ));
    }

    #[test]
    fn detects_responses_image_generation_tool() {
        let body = Bytes::from_static(
            br#"{"model":"demo","input":"draw","tools":[{"type":"image_generation"}]}"#,
        );

        assert!(uses_image_generation_tool(&body));
    }

    #[test]
    fn ignores_non_image_generation_tools() {
        let body = Bytes::from_static(
            br#"{"model":"demo","input":"lookup","tools":[{"type":"function","name":"lookup"}]}"#,
        );

        assert!(!uses_image_generation_tool(&body));
    }

    #[test]
    fn responses_string_input_converts_to_chat_messages() {
        let body = Bytes::from_static(
            br#"{"model":"demo","instructions":"Be concise","input":"hello","max_output_tokens":20,"text":{"format":{"type":"json_schema","name":"Answer","schema":{"type":"object"},"strict":true}}}"#,
        );

        let converted = responses_to_chat_body(&body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["model"], "demo");
        assert_eq!(value["stream"], false);
        assert_eq!(value["max_tokens"], 20);
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][0]["content"], "Be concise");
        assert_eq!(value["messages"][1]["role"], "user");
        assert_eq!(value["messages"][1]["content"], "hello");
        assert_eq!(value["response_format"]["type"], "json_schema");
        assert_eq!(value["response_format"]["json_schema"]["name"], "Answer");
    }

    #[test]
    fn responses_messages_tools_and_tool_choice_convert_to_chat_shape() {
        let body = Bytes::from_static(
            br#"{
                "model":"demo",
                "input":[{
                    "type":"message",
                    "role":"user",
                    "content":[
                        {"type":"input_text","text":"describe"},
                        {"type":"input_image","image_url":"https://example.test/a.png"}
                    ]
                }],
                "tools":[{"type":"function","name":"lookup","description":"Lookup","parameters":{"type":"object"},"strict":true}],
                "tool_choice":{"type":"function","name":"lookup"}
            }"#,
        );

        let converted = responses_to_chat_body(&body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["messages"][0]["content"][0]["type"], "text");
        assert_eq!(value["messages"][0]["content"][0]["text"], "describe");
        assert_eq!(value["messages"][0]["content"][1]["type"], "image_url");
        assert_eq!(
            value["messages"][0]["content"][1]["image_url"]["url"],
            "https://example.test/a.png"
        );
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["function"]["name"], "lookup");
        assert_eq!(value["tools"][0]["function"]["strict"], true);
        assert_eq!(value["tool_choice"]["function"]["name"], "lookup");
    }

    #[test]
    fn response_function_call_history_converts_to_chat_tool_messages() {
        let body = Bytes::from_static(
            br#"{
                "model":"demo",
                "input":[
                    {"type":"function_call","call_id":"call_1","name":"lookup","arguments":{"city":"Paris"}},
                    {"type":"function_call_output","call_id":"call_1","output":{"temp":21}}
                ]
            }"#,
        );

        let converted = responses_to_chat_body(&body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["messages"][0]["role"], "assistant");
        assert_eq!(value["messages"][0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            value["messages"][0]["tool_calls"][0]["function"]["arguments"],
            r#"{"city":"Paris"}"#
        );
        assert_eq!(value["messages"][1]["role"], "tool");
        assert_eq!(value["messages"][1]["tool_call_id"], "call_1");
        assert_eq!(value["messages"][1]["content"], r#"{"temp":21}"#);
    }

    #[test]
    fn chat_completion_response_converts_to_responses_shape() {
        let body = Bytes::from_static(
            br#"{
                "id":"chatcmpl_1",
                "object":"chat.completion",
                "created":1710000000,
                "model":"demo",
                "choices":[{
                    "index":0,
                    "message":{
                        "role":"assistant",
                        "content":"hi",
                        "tool_calls":[{
                            "id":"call_1",
                            "type":"function",
                            "function":{"name":"lookup","arguments":"{\"city\":\"Paris\"}"}
                        }]
                    },
                    "finish_reason":"tool_calls"
                }],
                "usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}
            }"#,
        );

        let converted = chat_to_responses_body(&body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["id"], "chatcmpl_1");
        assert_eq!(value["object"], "response");
        assert_eq!(value["created_at"], 1710000000);
        assert_eq!(value["status"], "completed");
        assert_eq!(value["output_text"], "hi");
        assert_eq!(value["output"][0]["type"], "message");
        assert_eq!(value["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(value["output"][1]["type"], "function_call");
        assert_eq!(value["output"][1]["call_id"], "call_1");
        assert_eq!(value["output"][1]["name"], "lookup");
        assert_eq!(value["output"][1]["arguments"], r#"{"city":"Paris"}"#);
        assert_eq!(value["usage"]["input_tokens"], 3);
        assert_eq!(value["usage"]["output_tokens"], 4);
        assert_eq!(value["usage"]["total_tokens"], 7);
    }
}
