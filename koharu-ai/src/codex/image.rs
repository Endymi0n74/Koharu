use serde::Serialize;

use super::responses::{CodexInputContent, CodexInputItem};

const DEFAULT_IMAGE_INSTRUCTIONS: &str = "Generate or edit the requested image.";
const DEFAULT_IMAGE_QUALITY: &str = "high";

#[derive(Debug, Clone, Serialize)]
pub struct CodexImageGenerationRequest {
    pub model: String,
    pub instructions: String,
    pub tools: [CodexImageGenerationTool; 1],
    pub input: Vec<CodexInputItem>,
    pub stream: bool,
    pub store: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexImageGenerationTool {
    #[serde(rename = "type")]
    pub tool_type: &'static str,
    #[serde(flatten)]
    pub image_generation: CodexImageGenerationConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexImageGenerationConfig {
    pub quality: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexInputImage {
    pub url: String,
    pub detail: String,
}

impl CodexImageGenerationRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            instructions: DEFAULT_IMAGE_INSTRUCTIONS.to_string(),
            tools: [CodexImageGenerationTool::default()],
            input: vec![CodexInputItem::user_text(prompt)],
            stream: true,
            store: false,
        }
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    pub fn with_quality(mut self, quality: impl Into<String>) -> Self {
        self.tools[0].image_generation.quality = quality.into();
        self
    }

    pub fn with_size(mut self, size: impl Into<String>) -> Self {
        self.tools[0].image_generation.size = Some(size.into());
        self
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.tools[0].image_generation.action = Some(action.into());
        self
    }

    pub fn with_input_image(mut self, image: CodexInputImage) -> Self {
        let content = CodexInputContent::input_image_url(image.url, Some(image.detail));
        if let Some(item) = self.input.first_mut() {
            item.content.push(content);
        } else {
            self.input.push(CodexInputItem {
                item_type: "message",
                role: "user",
                content: vec![content],
            });
        }
        self
    }
}

impl CodexImageGenerationTool {
    pub fn new(image_generation: CodexImageGenerationConfig) -> Self {
        Self {
            tool_type: "image_generation",
            image_generation,
        }
    }
}

impl Default for CodexImageGenerationTool {
    fn default() -> Self {
        Self::new(CodexImageGenerationConfig::default())
    }
}

impl Default for CodexImageGenerationConfig {
    fn default() -> Self {
        Self {
            quality: DEFAULT_IMAGE_QUALITY.to_string(),
            size: None,
            action: None,
        }
    }
}

impl CodexInputImage {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            detail: "high".to_string(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_image_generation_request_without_input_image() {
        let request = CodexImageGenerationRequest::new("gpt-image-2", "draw a koharu logo")
            .with_action("generate");
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["model"], "gpt-image-2");
        assert_eq!(value["instructions"], DEFAULT_IMAGE_INSTRUCTIONS);
        assert!(value["input"].is_array());
        assert_eq!(value["input"][0]["type"], "message");
        assert_eq!(value["input"][0]["role"], "user");
        assert_eq!(value["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(
            value["input"][0]["content"][0]["text"],
            "draw a koharu logo"
        );
        assert_eq!(value["tools"][0]["type"], "image_generation");
        assert_eq!(value["tools"][0]["quality"], "high");
        assert_eq!(value["tools"][0]["action"], "generate");
        assert_eq!(value["stream"], true);
        assert_eq!(value["store"], false);
        assert!(value.get("input_image").is_none());
    }

    #[test]
    fn serializes_image_generation_request_with_input_image() {
        let request = CodexImageGenerationRequest::new("gpt-image-2", "make it manga")
            .with_action("edit")
            .with_input_image(
                CodexInputImage::new("data:image/png;base64,abc").with_detail("high"),
            );
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(
            value["input"][0]["content"][1]["image_url"],
            "data:image/png;base64,abc"
        );
        assert_eq!(value["input"][0]["content"][1]["detail"], "high");
        assert_eq!(value["tools"][0]["action"], "edit");
        assert!(value.get("input_image").is_none());
    }
}
