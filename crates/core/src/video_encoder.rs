//! Hardware-agnostic H.264 video encoder boundary.
//!
//! `H264VideoEncoder` is the small surface that real and synthetic
//! encoders share. The default in-memory implementation
//! ([`InMemoryH264VideoEncoder`]) returns empty payloads, which keeps
//! the existing in-memory video stream pipeline state machine behaving
//! exactly as before. Concrete platform encoders (e.g. the macOS
//! VideoToolbox encoder behind the `macos-videotoolbox` cargo feature)
//! plug into the same trait but populate
//! `EncodedH264Frame::payload` with the real encoded bitstream.
//!
//! The trait deliberately does not name a frame source type. Real
//! encoders accept platform-specific buffers (e.g. `CVPixelBufferRef`
//! on macOS) through their concrete API surface so that the trait
//! itself stays platform-neutral and lives in `crates/core` without
//! pulling in OS bindings.

use apprelay_protocol::AppRelayError;

/// Configuration applied at encoder construction or reconfiguration
/// time. Validation lives in [`H264EncoderConfig::validate`] so every
/// implementation enforces the same minimums.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct H264EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub target_bitrate_kbps: u32,
    pub max_fps: u32,
    pub keyframe_interval_frames: u32,
}

impl H264EncoderConfig {
    /// Reject configurations that would make the underlying encoder
    /// silently misbehave. Returning a typed error keeps the
    /// "no silent no-ops" invariant from `CLAUDE.md`.
    pub fn validate(&self) -> Result<(), AppRelayError> {
        if self.width == 0 || self.height == 0 {
            return Err(AppRelayError::InvalidRequest(format!(
                "H.264 encoder dimensions must be non-zero (got {}x{})",
                self.width, self.height
            )));
        }
        if !self.width.is_multiple_of(2) || !self.height.is_multiple_of(2) {
            return Err(AppRelayError::InvalidRequest(format!(
                "H.264 encoder dimensions must be even (got {}x{}); H.264 macroblocks require it",
                self.width, self.height
            )));
        }
        if self.max_fps == 0 {
            return Err(AppRelayError::InvalidRequest(
                "H.264 encoder max_fps must be greater than zero".to_string(),
            ));
        }
        if self.target_bitrate_kbps == 0 {
            return Err(AppRelayError::InvalidRequest(
                "H.264 encoder target_bitrate_kbps must be greater than zero".to_string(),
            ));
        }
        if self.keyframe_interval_frames == 0 {
            return Err(AppRelayError::InvalidRequest(
                "H.264 encoder keyframe_interval_frames must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

/// One encoded H.264 NAL unit set in Annex-B framing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedH264Frame {
    pub payload: Vec<u8>,
    pub keyframe: bool,
    /// Encoder-side capture timestamp in milliseconds. The video
    /// stream pipeline uses this to populate `EncodedVideoFrame`.
    pub timestamp_ms: u64,
}

/// Minimum H.264 encoder surface used by the video stream pipeline.
pub trait H264VideoEncoder: Send + Sync {
    /// Apply a new configuration. Implementations may rebuild any
    /// underlying compression session; callers should treat this as
    /// expensive and avoid calling it per frame.
    fn configure(&mut self, config: H264EncoderConfig) -> Result<(), AppRelayError>;

    /// Cleanly shut down. Implementations must release any platform
    /// resources (sessions, completion handlers) here even if they
    /// have already been torn down.
    fn shutdown(&mut self);
}

/// Pure-Rust default encoder. It validates configuration but never
/// produces payload bytes, so existing in-memory video stream tests
/// continue to observe `payload: Vec::new()`.
#[derive(Clone, Debug, Default)]
pub struct InMemoryH264VideoEncoder {
    config: Option<H264EncoderConfig>,
    frames_pushed: u64,
}

impl InMemoryH264VideoEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of frames the in-memory encoder has been asked to encode
    /// since the last `configure` or `shutdown`. Provided for tests
    /// that want to assert the encoder was driven without inspecting
    /// any payload bytes.
    pub fn frames_pushed(&self) -> u64 {
        self.frames_pushed
    }

    /// Drive a synthetic frame through the encoder. The in-memory
    /// implementation always returns an empty payload. Returns
    /// `ServiceUnavailable` if `configure` has not been called yet,
    /// matching the contract real encoders honour.
    pub fn encode_synthetic_frame(
        &mut self,
        timestamp_ms: u64,
    ) -> Result<EncodedH264Frame, AppRelayError> {
        let config = self.config.as_ref().ok_or_else(|| {
            AppRelayError::ServiceUnavailable(
                "H.264 encoder must be configured before pushing frames".to_string(),
            )
        })?;
        self.frames_pushed += 1;
        let keyframe = self.frames_pushed == 1
            || (self.frames_pushed - 1).is_multiple_of(u64::from(config.keyframe_interval_frames));
        Ok(EncodedH264Frame {
            payload: Vec::new(),
            keyframe,
            timestamp_ms,
        })
    }
}

impl H264VideoEncoder for InMemoryH264VideoEncoder {
    fn configure(&mut self, config: H264EncoderConfig) -> Result<(), AppRelayError> {
        config.validate()?;
        self.config = Some(config);
        self.frames_pushed = 0;
        Ok(())
    }

    fn shutdown(&mut self) {
        self.config = None;
        self.frames_pushed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_config() -> H264EncoderConfig {
        H264EncoderConfig {
            width: 1280,
            height: 720,
            target_bitrate_kbps: 2_500,
            max_fps: 30,
            keyframe_interval_frames: 60,
        }
    }

    #[test]
    fn validate_rejects_zero_dimensions() {
        let mut config = ok_config();
        config.width = 0;
        assert!(matches!(
            config.validate(),
            Err(AppRelayError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_rejects_odd_dimensions() {
        let mut config = ok_config();
        config.width = 1281;
        assert!(matches!(
            config.validate(),
            Err(AppRelayError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_rejects_zero_fps() {
        let mut config = ok_config();
        config.max_fps = 0;
        assert!(matches!(
            config.validate(),
            Err(AppRelayError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_rejects_zero_bitrate() {
        let mut config = ok_config();
        config.target_bitrate_kbps = 0;
        assert!(matches!(
            config.validate(),
            Err(AppRelayError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_rejects_zero_keyframe_interval() {
        let mut config = ok_config();
        config.keyframe_interval_frames = 0;
        assert!(matches!(
            config.validate(),
            Err(AppRelayError::InvalidRequest(_))
        ));
    }

    #[test]
    fn in_memory_encoder_requires_configure_before_pushing_frames() {
        let mut encoder = InMemoryH264VideoEncoder::new();
        let err = encoder
            .encode_synthetic_frame(0)
            .expect_err("must require configure first");
        assert!(matches!(err, AppRelayError::ServiceUnavailable(_)));
    }

    #[test]
    fn in_memory_encoder_emits_empty_payloads_with_keyframe_cadence() {
        let mut encoder = InMemoryH264VideoEncoder::new();
        encoder.configure(ok_config()).expect("configure");

        let first = encoder.encode_synthetic_frame(0).expect("first frame");
        assert!(first.keyframe, "first frame must be a keyframe");
        assert!(first.payload.is_empty());

        for offset in 1u32..60 {
            let frame = encoder
                .encode_synthetic_frame(u64::from(offset) * 33)
                .expect("intermediate frame");
            assert!(!frame.keyframe, "frame {offset} should not be a keyframe");
        }

        let sixty_first = encoder.encode_synthetic_frame(60 * 33).expect("60th frame");
        assert!(
            sixty_first.keyframe,
            "the keyframe-interval boundary must produce a keyframe"
        );
        assert_eq!(encoder.frames_pushed(), 61);
    }

    #[test]
    fn in_memory_encoder_resets_state_on_shutdown() {
        let mut encoder = InMemoryH264VideoEncoder::new();
        encoder.configure(ok_config()).expect("configure");
        encoder.encode_synthetic_frame(0).expect("frame");
        assert_eq!(encoder.frames_pushed(), 1);

        encoder.shutdown();
        assert_eq!(encoder.frames_pushed(), 0);

        let err = encoder
            .encode_synthetic_frame(0)
            .expect_err("must be unconfigured after shutdown");
        assert!(matches!(err, AppRelayError::ServiceUnavailable(_)));
    }
}
