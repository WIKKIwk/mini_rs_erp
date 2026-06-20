use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct GeminiResponse {
    #[serde(default)]
    pub(super) candidates: Vec<GeminiCandidate>,
    pub(super) error: Option<GeminiError>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiCandidate {
    pub(super) content: GeminiContent,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiContent {
    #[serde(default)]
    pub(super) parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiPart {
    #[serde(default)]
    pub(super) text: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiError {
    pub(super) message: String,
}
