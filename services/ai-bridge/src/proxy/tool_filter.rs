use std::{collections::HashMap, sync::Arc};

use aho_corasick::AhoCorasick;
use anyhow::{Context, Result};
use bytes::Bytes;
use serde_json::Value;

use crate::{auth::ApiKeyInfo, config::Config, logger};

use super::types::{RequestMetadata, ResponseTextMetadata};

const TOOL_FILTER_TOOLS_FIELD_MARKER: &str = r#""tools""#;
const TOOL_FILTER_TOOLS_FIELD_PATTERN: usize = 0;

#[derive(Debug)]
pub(super) struct ToolFilterIndex {
    model_prefixes: Vec<String>,
    body_marker_matcher: Option<AhoCorasick>,
    scan_limit: usize,
}

impl ToolFilterIndex {
    pub(super) fn new(config: &HashMap<String, Vec<String>>, scan_limit: usize) -> Result<Self> {
        let mut model_prefixes: Vec<String> = config.keys().cloned().collect();
        model_prefixes.sort_unstable();

        let body_marker_matcher = build_tool_filter_body_marker_matcher(config)?;

        Ok(Self {
            model_prefixes,
            body_marker_matcher,
            scan_limit,
        })
    }

    pub(super) fn body_may_need_filter(&self, body: &[u8]) -> bool {
        let Some(matcher) = &self.body_marker_matcher else {
            return false;
        };

        let mut found_tools_field = false;
        let mut found_configured_tool_type = false;

        // scan_limit is an early-exit window, not a correctness boundary. If the
        // prefix is not decisive, continue scanning the remaining bytes so large
        // prompts with tools near the end still get filtered.
        let prefix_len = self.scan_limit.min(body.len());
        if prefix_len > 0
            && scan_tool_filter_markers(
                matcher,
                &body[..prefix_len],
                &mut found_tools_field,
                &mut found_configured_tool_type,
            )
        {
            return true;
        }

        prefix_len < body.len()
            && scan_tool_filter_markers(
                matcher,
                &body[prefix_len..],
                &mut found_tools_field,
                &mut found_configured_tool_type,
            )
    }

    pub(super) fn model_may_need_filter(&self, model: &str) -> bool {
        self.model_prefixes
            .iter()
            .any(|prefix| model.starts_with(prefix.as_str()))
    }

    pub(super) fn tools_to_filter_for_model<'a>(
        &self,
        model: &str,
        config: &'a HashMap<String, Vec<String>>,
    ) -> Vec<&'a str> {
        let mut tools_to_filter: Vec<&str> = self
            .model_prefixes
            .iter()
            .filter(|prefix| model.starts_with(prefix.as_str()))
            .filter_map(|prefix| config.get(prefix.as_str()))
            .flat_map(|tools| tools.iter().map(String::as_str))
            .collect();

        tools_to_filter.sort_unstable();
        tools_to_filter.dedup();
        tools_to_filter
    }
}

fn scan_tool_filter_markers(
    matcher: &AhoCorasick,
    scan_slice: &[u8],
    found_tools_field: &mut bool,
    found_configured_tool_type: &mut bool,
) -> bool {
    for mat in matcher.find_iter(scan_slice) {
        if mat.pattern().as_usize() == TOOL_FILTER_TOOLS_FIELD_PATTERN {
            *found_tools_field = true;
        } else {
            *found_configured_tool_type = true;
        }

        if *found_tools_field && *found_configured_tool_type {
            return true;
        }
    }

    false
}

fn build_tool_filter_body_marker_matcher(
    config: &HashMap<String, Vec<String>>,
) -> Result<Option<AhoCorasick>> {
    if config.is_empty() {
        return Ok(None);
    }

    let mut patterns = Vec::new();
    patterns.push(TOOL_FILTER_TOOLS_FIELD_MARKER.to_owned());

    let mut tool_types: Vec<String> = config
        .values()
        .flat_map(|tools| tools.iter())
        .filter(|tool_type| {
            !tool_type.is_empty() && tool_type.as_str() != TOOL_FILTER_TOOLS_FIELD_MARKER
        })
        .cloned()
        .collect();
    tool_types.sort_unstable();
    tool_types.dedup();

    if tool_types.is_empty() {
        return Ok(None);
    }

    patterns.extend(tool_types);

    AhoCorasick::builder()
        .build(patterns)
        .map(Some)
        .context("failed to build tool filter body marker matcher")
}

pub(super) fn request_body_may_need_tool_filter(
    body: &Bytes,
    tool_filter_index: &ToolFilterIndex,
) -> bool {
    tool_filter_index.body_may_need_filter(body)
}

pub(super) fn request_metadata_needs_structured_output_normalization(
    meta: &RequestMetadata<'_>,
    path: &str,
) -> bool {
    meta.response_format.is_some()
        || (path == "/v1/responses"
            && meta
                .text
                .as_ref()
                .is_some_and(ResponseTextMetadata::has_format))
}

pub(super) fn request_metadata_needs_responses_compat_normalization(
    meta: &RequestMetadata<'_>,
    path: &str,
) -> bool {
    path == "/v1/responses" && meta.max_tokens.is_some()
}

pub(super) fn normalize_responses_compat_request_value(value: &mut Value) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    let Some(max_tokens) = object.remove("max_tokens") else {
        return false;
    };

    object.entry("max_output_tokens").or_insert(max_tokens);
    true
}

pub(super) fn request_metadata_needs_tool_filter(
    meta: &RequestMetadata<'_>,
    tool_filter_index: &ToolFilterIndex,
    config: &HashMap<String, Vec<String>>,
) -> bool {
    let Some(model) = meta.model.as_deref() else {
        return false;
    };
    if !tool_filter_index.model_may_need_filter(model) {
        return false;
    }

    let tools_to_filter = tool_filter_index.tools_to_filter_for_model(model, config);
    if tools_to_filter.is_empty() {
        return false;
    }

    meta.tools
        .as_ref()
        .is_some_and(|tools| tools.contains_any_type(&tools_to_filter))
}

/// 根据配置过滤请求体中的工具调用。
///
/// 返回修改后的请求体，如果无需修改则返回 None。
#[cfg(test)]
pub(super) fn filter_tools_by_model(
    body: &Bytes,
    config: &HashMap<String, Vec<String>>,
    tool_filter_index: &ToolFilterIndex,
    config_arc: &Arc<Config>,
    api_key: &ApiKeyInfo,
    path: &str,
) -> Option<Bytes> {
    if !request_body_may_need_tool_filter(body, tool_filter_index) {
        return None;
    }

    let mut json: Value = match serde_json::from_slice(body) {
        Ok(json) => json,
        Err(_) => return None, // 不是有效 JSON，跳过过滤
    };

    if filter_tools_json_by_model(
        &mut json,
        config,
        tool_filter_index,
        config_arc,
        api_key,
        path,
    ) {
        serde_json::to_vec(&json).ok().map(Bytes::from)
    } else {
        None
    }
}

pub(super) fn filter_tools_json_by_model(
    json: &mut Value,
    config: &HashMap<String, Vec<String>>,
    tool_filter_index: &ToolFilterIndex,
    config_arc: &Arc<Config>,
    api_key: &ApiKeyInfo,
    path: &str,
) -> bool {
    let Some(model) = json.get("model").and_then(Value::as_str) else {
        return false;
    };

    let tools_to_filter = tool_filter_index.tools_to_filter_for_model(model, config);
    if tools_to_filter.is_empty() {
        return false;
    }

    // 在进行可变借用之前，先克隆 model（只在确定需要过滤时才克隆）
    let model_owned = model.to_owned();

    // 获取 tools 数组（如果不存在则无需过滤）
    let Some(tools_array) = json.get_mut("tools").and_then(Value::as_array_mut) else {
        return false;
    };

    let original_count = tools_array.len();

    // 过滤掉配置中指定的工具类型
    tools_array.retain(|tool| {
        let tool_type = tool.get("type").and_then(|t| t.as_str());
        tool_type.is_none_or(|t| tools_to_filter.binary_search(&t).is_err())
    });

    let filtered_count = tools_array.len();

    // 快速路径：没有过滤任何工具
    if original_count == filtered_count {
        return false;
    }

    // 记录调试日志（只在确实过滤了工具时才记录）
    if config_arc.debug_output {
        logger::log_debug_operation(
            config_arc,
            api_key,
            "POST",
            path,
            "tools_filtered",
            format!(
                "model={} filtered_types={:?} original_count={} filtered_count={}",
                model_owned, tools_to_filter, original_count, filtered_count
            ),
        );
    }

    // 如果过滤后 tools 为空，移除整个 tools 字段
    if tools_array.is_empty() {
        let Some(object) = json.as_object_mut() else {
            return false;
        };
        object.remove("tools");
    }

    true
}
