//! Integration test for `VideoToolboxH264Encoder`.
//!
//! Drives the real `VTCompressionSession` end-to-end with a synthetic
//! `CVPixelBuffer` (a flat colour ramp) and asserts that the encoder
//! emits at least one non-empty H.264 Annex-B payload through the
//! sink, with the first frame marked as a keyframe.
//!
//! It is `#[ignore]` because it requires:
//!   * macOS host (compile-time gated)
//!   * the `macos-videotoolbox` cargo feature (compile-time gated)
//!
//! Unlike the ScreenCaptureKit integration test, no Screen Recording
//! permission is required: we synthesise the input pixel buffer
//! ourselves so the framework never needs to look at the display.
//!
//! Run locally with:
//!   cargo test -p apprelay-core --features macos-videotoolbox \
//!     --test macos_videotoolbox -- --ignored

#![cfg(all(feature = "macos-videotoolbox", target_os = "macos"))]

use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use apprelay_core::{
    EncodedH264Frame, H264EncoderConfig, H264VideoEncoder, VideoToolboxEncodedFrameSink,
    VideoToolboxH264Encoder,
};
use apprelay_protocol::AppRelayError;

#[allow(non_camel_case_types)]
type CVPixelBufferRef = *mut c_void;
#[allow(non_camel_case_types)]
type CVReturn = i32;
#[allow(non_camel_case_types)]
type CFAllocatorRef = *const c_void;
#[allow(non_camel_case_types)]
type CFDictionaryRef = *const c_void;
#[allow(non_camel_case_types)]
type OSType = u32;

const K_CV_PIXEL_FORMAT_TYPE_32BGRA: OSType = u32::from_be_bytes(*b"BGRA");

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVPixelBufferCreate(
        allocator: CFAllocatorRef,
        width: usize,
        height: usize,
        pixel_format_type: OSType,
        pixel_buffer_attributes: CFDictionaryRef,
        pixel_buffer_out: *mut CVPixelBufferRef,
    ) -> CVReturn;

    fn CVPixelBufferLockBaseAddress(pixel_buffer: CVPixelBufferRef, lock_flags: u64) -> CVReturn;

    fn CVPixelBufferUnlockBaseAddress(pixel_buffer: CVPixelBufferRef, lock_flags: u64) -> CVReturn;

    fn CVPixelBufferGetBaseAddress(pixel_buffer: CVPixelBufferRef) -> *mut c_void;

    fn CVPixelBufferGetBytesPerRow(pixel_buffer: CVPixelBufferRef) -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

#[derive(Default)]
struct CapturingSink {
    frames: Mutex<Vec<EncodedH264Frame>>,
    errors: Mutex<Vec<AppRelayError>>,
}

impl VideoToolboxEncodedFrameSink for CapturingSink {
    fn on_encoded_frame(&self, frame: EncodedH264Frame) {
        self.frames
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(frame);
    }

    fn on_encode_error(&self, error: AppRelayError) {
        self.errors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(error);
    }
}

fn make_synthetic_pixel_buffer(width: usize, height: usize, sequence: u8) -> CVPixelBufferRef {
    let mut pixel_buffer: CVPixelBufferRef = ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreate(
            ptr::null(),
            width,
            height,
            K_CV_PIXEL_FORMAT_TYPE_32BGRA,
            ptr::null(),
            &mut pixel_buffer,
        )
    };
    assert_eq!(status, 0, "CVPixelBufferCreate failed with status {status}");
    assert!(!pixel_buffer.is_null(), "CVPixelBufferCreate returned null");

    // Fill with a recognisable per-frame colour so we are not feeding
    // identical buffers to the encoder. The encoder tolerates static
    // input but is more representative when the bytes change.
    unsafe {
        assert_eq!(CVPixelBufferLockBaseAddress(pixel_buffer, 0), 0);
        let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *mut u8;
        let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
        for y in 0..height {
            let row = base.add(y * bytes_per_row);
            for x in 0..width {
                let pixel = row.add(x * 4);
                // BGRA components vary with x, y, and sequence so each
                // frame is unique.
                *pixel.add(0) = ((x + sequence as usize) % 256) as u8; // B
                *pixel.add(1) = ((y + sequence as usize) % 256) as u8; // G
                *pixel.add(2) = sequence; // R
                *pixel.add(3) = 0xff; // A
            }
        }
        assert_eq!(CVPixelBufferUnlockBaseAddress(pixel_buffer, 0), 0);
    }

    pixel_buffer
}

#[test]
#[ignore = "requires macOS VideoToolbox (no Screen Recording permission needed)"]
fn videotoolbox_encoder_emits_real_h264_payload() {
    let sink = Arc::new(CapturingSink::default());
    let mut encoder = VideoToolboxH264Encoder::new(sink.clone());

    let config = H264EncoderConfig {
        width: 320,
        height: 240,
        target_bitrate_kbps: 500,
        max_fps: 30,
        keyframe_interval_frames: 30,
    };
    encoder.configure(config).expect("configure encoder");

    // Submit ~15 frames at 33 ms apart so the encoder has enough input
    // to emit at least one keyframe and one delta frame.
    let frame_count: usize = 15;
    let mut pixel_buffers = Vec::with_capacity(frame_count);
    for index in 0..frame_count {
        let pb = make_synthetic_pixel_buffer(320, 240, index as u8);
        unsafe {
            encoder
                .encode_pixel_buffer(pb, (index as u64) * 33)
                .expect("encode_pixel_buffer");
        }
        pixel_buffers.push(pb);
    }

    encoder.complete_frames().expect("complete_frames");

    // VideoToolbox callbacks may run on a worker thread; give them a
    // short window to drain before asserting.
    thread::sleep(Duration::from_millis(100));

    let frames = sink
        .frames
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let errors = sink
        .errors
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(errors.is_empty(), "encoder reported errors: {errors:?}");
    assert!(
        !frames.is_empty(),
        "VideoToolbox should have emitted at least one frame"
    );
    let first = &frames[0];
    assert!(first.keyframe, "first frame must be a keyframe");
    assert!(
        !first.payload.is_empty(),
        "first frame payload must be non-empty"
    );
    // Annex-B sync NAL units start with a 4-byte start code followed
    // by a parameter-set NAL (type 7, SPS) on a keyframe.
    assert_eq!(
        first.payload.get(0..4),
        Some(&[0u8, 0, 0, 1][..]),
        "Annex-B start code must be 0x00 0x00 0x00 0x01"
    );

    // Drop encoder explicitly so any background callbacks settle
    // before we release the synthetic pixel buffers.
    drop(encoder);
    for pb in pixel_buffers {
        unsafe { CFRelease(pb as *const c_void) };
    }
}
