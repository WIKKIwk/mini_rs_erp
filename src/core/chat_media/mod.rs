mod models;
mod ports;
mod processor;
mod processor_video;
mod service;
mod service_chunks;
mod service_processing;
mod service_support;
mod unavailable;

pub use models::*;
pub use ports::*;
pub use processor::SystemChatMediaProcessor;
pub use service::{
    ChatMediaService, DEFAULT_CHAT_VIDEO_CHUNK_SIZE_BYTES,
    MAX_CHAT_IMAGE_SIZE_BYTES, MAX_CHAT_MEDIA_CHUNK_SIZE_BYTES,
    MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES, MAX_CHAT_VIDEO_DURATION_MS,
    MAX_CHAT_VIDEO_SIZE_BYTES,
};

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
