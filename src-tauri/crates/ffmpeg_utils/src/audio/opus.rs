//! The two Opus profiles used by previews and subtitle-service uploads.

use std::path::Path;

use super::encode::{encode, encode_with_timing, EncodeProfile};

/// Encode a 16 kHz mono 64 kbit/s VBR Opus preview.
pub async fn encode_opus_sample(input: &Path, output: &Path) -> Result<(), String> {
    encode(input, output, EncodeProfile::OpusSample).await
}

/// Encode a 128 kbit/s VBR Opus upload with source-adaptive rate and layout.
pub async fn encode_opus_upload(input: &Path, output: &Path) -> Result<(), String> {
    encode(input, output, EncodeProfile::OpusUpload).await
}

/// A material sidecar on the playback clock, padding leading delay/gaps and
/// trimming overlaps so sample-based subtitle services see the video timeline.
pub async fn encode_opus_sample_timeline(input: &Path, output: &Path) -> Result<(), String> {
    encode_with_timing(input, output, EncodeProfile::OpusSample, true).await
}

/// Source-adaptive upload with the same playback-clock policy as sidecars.
pub async fn encode_opus_upload_timeline(input: &Path, output: &Path) -> Result<(), String> {
    encode_with_timing(input, output, EncodeProfile::OpusUpload, true).await
}
