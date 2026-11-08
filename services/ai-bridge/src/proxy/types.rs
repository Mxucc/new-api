use std::borrow::Cow;

/// 请求元数据结构体，用于轻量级 JSON 解析。
///
/// 只解析需要检查或修改的字段（model、tools[].type、response_format、text.format、max_tokens）。
/// 大字段（messages、prompt、system、input）会被 serde 自动忽略；tools 只抽取
/// 每个工具的 type 字段，response_format、text.format 和 max_tokens 只判断字段存在。
#[derive(serde::Deserialize)]
pub(super) struct RequestMetadata<'a> {
    #[serde(borrow)]
    pub(super) model: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub(super) tools: Option<RequestToolsMetadata<'a>>,
    #[serde(default)]
    pub(super) response_format: Option<serde::de::IgnoredAny>,
    #[serde(default)]
    pub(super) text: Option<ResponseTextMetadata>,
    #[serde(default)]
    pub(super) max_tokens: Option<serde::de::IgnoredAny>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub(super) enum RequestToolsMetadata<'a> {
    Array(#[serde(borrow)] Vec<RequestToolMetadata<'a>>),
    Other(serde::de::IgnoredAny),
}

impl RequestToolsMetadata<'_> {
    pub(super) fn contains_any_type(&self, tool_types: &[&str]) -> bool {
        let Self::Array(tools) = self else {
            return false;
        };

        tools.iter().any(|tool| {
            tool.tool_type()
                .is_some_and(|tool_type| tool_types.binary_search(&tool_type).is_ok())
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub(super) enum RequestToolMetadata<'a> {
    Object {
        #[serde(rename = "type", default, borrow)]
        tool_type: Option<RequestToolType<'a>>,
    },
    Other(serde::de::IgnoredAny),
}

impl RequestToolMetadata<'_> {
    pub(super) fn tool_type(&self) -> Option<&str> {
        match self {
            Self::Object {
                tool_type: Some(RequestToolType::String(tool_type)),
            } => Some(tool_type.as_ref()),
            Self::Object { .. } | Self::Other(_) => None,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub(super) enum RequestToolType<'a> {
    String(#[serde(borrow)] Cow<'a, str>),
    Other(serde::de::IgnoredAny),
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub(super) enum ResponseTextMetadata {
    Object {
        #[serde(default)]
        format: Option<serde::de::IgnoredAny>,
    },
    Other(serde::de::IgnoredAny),
}

impl ResponseTextMetadata {
    pub(super) fn has_format(&self) -> bool {
        match self {
            Self::Object { format } => format.is_some(),
            Self::Other(_) => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct RequestBodyInspectionPlan {
    pub(super) should_check_image_model: bool,
    pub(super) should_normalize_structured_output: bool,
    pub(super) should_normalize_responses_compat: bool,
    pub(super) should_filter_tools: bool,
}

impl RequestBodyInspectionPlan {
    pub(super) fn needs_metadata_parse(self) -> bool {
        self.should_check_image_model
            || self.should_normalize_structured_output
            || self.should_normalize_responses_compat
            || self.should_filter_tools
    }
}
