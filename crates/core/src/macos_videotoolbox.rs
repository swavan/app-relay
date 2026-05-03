//! VideoToolbox-backed H.264 hardware encoder for macOS.
//!
//! This module wraps Apple's `VTCompressionSession` so that captured
//! pixel buffers (typically the `CVImageBuffer` inside a
//! `CMSampleBuffer` produced by `ScreenCaptureKit`) can be turned into
//! an H.264 elementary stream in Annex-B framing.
//!
//! ### Why thin FFI instead of a higher-level wrapper
//!
//! The VideoToolbox C surface used here is small (six functions,
//! a handful of CFString property keys) and the bytes we ship across
//! the network must be in a very specific framing (Annex-B, with the
//! SPS/PPS in-band on every keyframe). Driving the framework directly
//! lets us own that conversion without taking a second native binding
//! crate just for `VTCompressionSession`. The unsafe surface is
//! confined to this file and called only when the
//! `macos-videotoolbox` cargo feature is enabled.
//!
//! ### Cross-cutting rules
//!
//! - Default Linux/Windows/macOS builds never compile this file.
//! - Every framework error is mapped to a typed [`AppRelayError`]:
//!   permission/entitlement issues to `PermissionDenied`, framework
//!   failures to `ServiceUnavailable`, missing inputs to
//!   `InvalidRequest` / `NotFound`. There is never a silent no-op.
//! - The encoder retains no references to caller-owned buffers; once
//!   `VTCompressionSessionEncodeFrame` returns, the input pixel buffer
//!   may be released.

#![cfg(all(feature = "macos-videotoolbox", target_os = "macos"))]

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::{Arc, Mutex};

use apprelay_protocol::AppRelayError;
use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::{kCFBooleanFalse, kCFBooleanTrue};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};

use crate::video_encoder::{EncodedH264Frame, H264EncoderConfig, H264VideoEncoder};
#[cfg(feature = "macos-screencapturekit")]
use crate::video_stream::{
    MacosWindowCaptureResizeRequest, MacosWindowCaptureRuntime, MacosWindowCaptureStartRequest,
};

// ------------------------------------------------------------------
// Raw FFI surface
// ------------------------------------------------------------------

#[allow(non_camel_case_types)]
type OSStatus = i32;
#[allow(non_camel_case_types)]
type CMVideoCodecType = u32;

const K_CMVIDEO_CODEC_TYPE_H264: CMVideoCodecType = u32::from_be_bytes(*b"avc1");

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

const K_CMTIME_FLAG_VALID: u32 = 1;

fn cm_time(value: i64, timescale: i32) -> CMTime {
    CMTime {
        value,
        timescale,
        flags: K_CMTIME_FLAG_VALID,
        epoch: 0,
    }
}

fn cm_time_invalid() -> CMTime {
    CMTime {
        value: 0,
        timescale: 0,
        flags: 0,
        epoch: 0,
    }
}

#[allow(non_camel_case_types)]
type VTCompressionSessionRef = *mut c_void;
#[allow(non_camel_case_types)]
type CVImageBufferRef = *mut c_void;
#[allow(non_camel_case_types)]
type CMSampleBufferRef = *mut c_void;
#[allow(non_camel_case_types)]
type CMBlockBufferRef = *mut c_void;
#[allow(non_camel_case_types)]
type CMFormatDescriptionRef = *mut c_void;
#[allow(non_camel_case_types)]
type CFAllocatorRef = *const c_void;
#[allow(non_camel_case_types)]
type CFDictionaryRef = *const c_void;
#[allow(non_camel_case_types)]
type CFArrayRef = *const c_void;
#[allow(non_camel_case_types)]
type VTEncodeInfoFlags = u32;

type VTCompressionOutputCallback = unsafe extern "C" fn(
    output_callback_ref_con: *mut c_void,
    source_frame_ref_con: *mut c_void,
    status: OSStatus,
    info_flags: VTEncodeInfoFlags,
    sample_buffer: CMSampleBufferRef,
);

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    fn VTCompressionSessionCreate(
        allocator: CFAllocatorRef,
        width: i32,
        height: i32,
        codec_type: CMVideoCodecType,
        encoder_specification: CFDictionaryRef,
        source_image_buffer_attributes: CFDictionaryRef,
        compressed_data_allocator: CFAllocatorRef,
        output_callback: Option<VTCompressionOutputCallback>,
        output_callback_ref_con: *mut c_void,
        compression_session_out: *mut VTCompressionSessionRef,
    ) -> OSStatus;

    fn VTCompressionSessionInvalidate(session: VTCompressionSessionRef);

    fn VTCompressionSessionEncodeFrame(
        session: VTCompressionSessionRef,
        image_buffer: CVImageBufferRef,
        presentation_time_stamp: CMTime,
        duration: CMTime,
        frame_properties: CFDictionaryRef,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut VTEncodeInfoFlags,
    ) -> OSStatus;

    fn VTCompressionSessionCompleteFrames(
        session: VTCompressionSessionRef,
        complete_until_presentation_time_stamp: CMTime,
    ) -> OSStatus;

    fn VTSessionSetProperty(
        session: VTCompressionSessionRef,
        property_key: CFStringRef,
        property_value: CFTypeRef,
    ) -> OSStatus;

    static kVTCompressionPropertyKey_RealTime: CFStringRef;
    static kVTCompressionPropertyKey_AverageBitRate: CFStringRef;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: CFStringRef;
    static kVTCompressionPropertyKey_ExpectedFrameRate: CFStringRef;
    static kVTCompressionPropertyKey_AllowFrameReordering: CFStringRef;
    static kVTCompressionPropertyKey_ProfileLevel: CFStringRef;
    static kVTProfileLevel_H264_Baseline_AutoLevel: CFStringRef;
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetDataBuffer(sample_buffer: CMSampleBufferRef) -> CMBlockBufferRef;
    fn CMSampleBufferGetFormatDescription(
        sample_buffer: CMSampleBufferRef,
    ) -> CMFormatDescriptionRef;
    fn CMSampleBufferGetSampleAttachmentsArray(
        sample_buffer: CMSampleBufferRef,
        create_if_necessary: u8,
    ) -> CFArrayRef;

    fn CMBlockBufferGetDataLength(block_buffer: CMBlockBufferRef) -> usize;
    fn CMBlockBufferCopyDataBytes(
        block_buffer: CMBlockBufferRef,
        offset_to_data: usize,
        data_length: usize,
        destination: *mut c_void,
    ) -> OSStatus;

    fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        video_desc: CMFormatDescriptionRef,
        parameter_set_index: usize,
        parameter_set_pointer_out: *mut *const u8,
        parameter_set_size_out: *mut usize,
        parameter_set_count_out: *mut usize,
        nal_unit_header_length_out: *mut i32,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: isize) -> *const c_void;
    fn CFDictionaryGetValue(dictionary: CFDictionaryRef, key: *const c_void) -> *const c_void;
}

// `kCMSampleAttachmentKey_NotSync` is exported from CoreMedia. When the
// attachment's value is `kCFBooleanTrue`, the frame is a delta frame;
// when it is absent or false, the frame is a sync frame (keyframe).
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    static kCMSampleAttachmentKey_NotSync: CFStringRef;
}

// ------------------------------------------------------------------
// Public encoder type
// ------------------------------------------------------------------

/// Trait the encoder uses to deliver completed H.264 NAL units. The
/// callback is invoked from VideoToolbox's encoder thread, so
/// implementations must not block for long and must be `Send + Sync`.
pub trait VideoToolboxEncodedFrameSink: Send + Sync + 'static {
    fn on_encoded_frame(&self, frame: EncodedH264Frame);
    fn on_encode_error(&self, error: AppRelayError);
}

/// `VTCompressionSession`-backed H.264 encoder.
///
/// Construct with [`VideoToolboxH264Encoder::new`], call
/// [`H264VideoEncoder::configure`] with target dimensions and rate
/// info, then drive frames in via
/// [`VideoToolboxH264Encoder::encode_pixel_buffer`]. Encoded NAL units
/// are delivered asynchronously to the configured sink.
///
/// Drop or [`H264VideoEncoder::shutdown`] to tear the session down.
pub struct VideoToolboxH264Encoder {
    state: Arc<EncoderState>,
}

struct EncoderState {
    /// `VTCompressionSession` lives behind a mutex because
    /// configuration changes (e.g. on resize) must wait for in-flight
    /// frames to drain before the session is replaced.
    session: Mutex<SessionSlot>,
    sink: Arc<dyn VideoToolboxEncodedFrameSink>,
    /// Tracks whether the next emitted frame should prepend the
    /// `SPS`/`PPS` parameter sets. We do this on the very first frame
    /// of every session and on every keyframe, which is the
    /// minimum required for clean Annex-B playback.
    last_sps_pps: Mutex<Vec<u8>>,
}

struct SessionSlot {
    session: Option<NonNull<c_void>>,
    config: Option<H264EncoderConfig>,
}

// SAFETY: `VTCompressionSessionRef` is reference-counted by
// CoreFoundation and is documented as thread-safe for encode/property
// calls. We additionally guard mutation with a Mutex.
unsafe impl Send for SessionSlot {}
unsafe impl Send for VideoToolboxH264Encoder {}
unsafe impl Sync for VideoToolboxH264Encoder {}

impl VideoToolboxH264Encoder {
    pub fn new(sink: Arc<dyn VideoToolboxEncodedFrameSink>) -> Self {
        Self {
            state: Arc::new(EncoderState {
                session: Mutex::new(SessionSlot {
                    session: None,
                    config: None,
                }),
                sink,
                last_sps_pps: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Encode a pixel buffer at the given presentation timestamp
    /// (milliseconds since some monotonic origin chosen by the
    /// caller). The pixel buffer must remain valid for the duration
    /// of this call; once it returns, VideoToolbox has retained any
    /// data it still needs.
    ///
    /// # Safety
    /// `pixel_buffer` must be a valid `CVPixelBufferRef`. Passing a
    /// dangling or null pointer will cause undefined behaviour inside
    /// VideoToolbox.
    pub unsafe fn encode_pixel_buffer(
        &self,
        pixel_buffer: CVImageBufferRef,
        timestamp_ms: u64,
    ) -> Result<(), AppRelayError> {
        if pixel_buffer.is_null() {
            return Err(AppRelayError::InvalidRequest(
                "VideoToolbox encoder received a null pixel buffer".to_string(),
            ));
        }

        let session_ptr = {
            let slot = self
                .state
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            slot.session.ok_or_else(|| {
                AppRelayError::ServiceUnavailable(
                    "VideoToolbox encoder must be configured before encode_pixel_buffer"
                        .to_string(),
                )
            })?
        };

        // Use a millisecond timescale so callers do not need to know
        // VideoToolbox's preferred timebase. 1 ms is sufficient for the
        // 30-60 fps capture path; switch to a finer timescale if the
        // capture pipeline ever needs sub-millisecond timing.
        let pts = cm_time(i64::try_from(timestamp_ms).unwrap_or(i64::MAX), 1_000);
        let duration = cm_time_invalid();

        let mut info_flags: VTEncodeInfoFlags = 0;
        // SAFETY: session pointer was created by VTCompressionSessionCreate
        // and has not been invalidated yet (we still hold a reference
        // through the SessionSlot mutex above).
        let status = VTCompressionSessionEncodeFrame(
            session_ptr.as_ptr(),
            pixel_buffer,
            pts,
            duration,
            ptr::null(),
            ptr::null_mut(),
            &mut info_flags,
        );

        if status != 0 {
            return Err(map_videotoolbox_status(
                "VTCompressionSessionEncodeFrame",
                status,
            ));
        }
        Ok(())
    }

    /// Block until VideoToolbox has flushed every queued frame. Useful
    /// for tests that assert callbacks completed before the encoder is
    /// torn down.
    pub fn complete_frames(&self) -> Result<(), AppRelayError> {
        let session_ptr = {
            let slot = self
                .state
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            slot.session
        };
        let Some(session_ptr) = session_ptr else {
            return Ok(());
        };
        // SAFETY: session is still alive (we hold an Arc to the
        // EncoderState; tear-down only happens on shutdown/drop).
        let status =
            unsafe { VTCompressionSessionCompleteFrames(session_ptr.as_ptr(), cm_time_invalid()) };
        if status != 0 {
            return Err(map_videotoolbox_status(
                "VTCompressionSessionCompleteFrames",
                status,
            ));
        }
        Ok(())
    }
}

impl H264VideoEncoder for VideoToolboxH264Encoder {
    fn configure(&mut self, config: H264EncoderConfig) -> Result<(), AppRelayError> {
        config.validate()?;

        // Tear the existing session down before installing a new one
        // so the callback context never refers to a stale Arc.
        self.tear_down_session();

        let mut new_session: VTCompressionSessionRef = ptr::null_mut();
        let callback_ctx = Arc::into_raw(Arc::clone(&self.state)) as *mut c_void;
        // SAFETY: passing nulls for optional dictionaries is documented
        // as accepted by VTCompressionSessionCreate. The callback ctx
        // is a freshly leaked Arc reference; we recover it on
        // tear_down_session.
        let status = unsafe {
            VTCompressionSessionCreate(
                ptr::null(),
                config.width as i32,
                config.height as i32,
                K_CMVIDEO_CODEC_TYPE_H264,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                Some(compression_output_callback),
                callback_ctx,
                &mut new_session,
            )
        };

        if status != 0 || new_session.is_null() {
            // Drop the leaked Arc reference if the framework rejected
            // the session, or we would leak it forever.
            // SAFETY: we just leaked this very Arc above; nothing else
            // has consumed it.
            unsafe {
                drop(Arc::from_raw(callback_ctx as *const EncoderState));
            }
            return Err(map_videotoolbox_status(
                "VTCompressionSessionCreate",
                status,
            ));
        }

        // SAFETY: VTCompressionSessionCreate returned a non-null,
        // non-invalid session per the status check above.
        let session_nn = unsafe { NonNull::new_unchecked(new_session) };

        // Apply tuning properties. Failures here are recoverable from
        // the framework's perspective (the session would still encode
        // with defaults), but for AppRelay we want explicit control,
        // so any failure is bubbled out and the partially-configured
        // session is invalidated.
        if let Err(error) = configure_session_properties(session_nn, &config) {
            unsafe {
                VTCompressionSessionInvalidate(session_nn.as_ptr());
                CFRelease(session_nn.as_ptr() as *const c_void);
                drop(Arc::from_raw(callback_ctx as *const EncoderState));
            }
            return Err(error);
        }

        let mut slot = self
            .state
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slot.session = Some(session_nn);
        slot.config = Some(config);
        Ok(())
    }

    fn shutdown(&mut self) {
        self.tear_down_session();
    }
}

impl VideoToolboxH264Encoder {
    fn tear_down_session(&mut self) {
        let mut slot = self
            .state
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(session) = slot.session.take() {
            // SAFETY: session is a valid VT session created above and
            // not yet invalidated. Invalidate releases the callback
            // context Arc reference indirectly: we recover the leaked
            // Arc here so its strong count returns to whatever it was
            // before configure() leaked it.
            unsafe {
                VTCompressionSessionCompleteFrames(session.as_ptr(), cm_time_invalid());
                VTCompressionSessionInvalidate(session.as_ptr());
                CFRelease(session.as_ptr() as *const c_void);
                // The callback context Arc we leaked in configure()
                // shares the same backing allocation as `self.state`.
                // We can recover it by reconstructing one strong ref.
                let raw = Arc::as_ptr(&self.state);
                drop(Arc::from_raw(raw));
            }
        }
        slot.config = None;
        self.state
            .last_sps_pps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

impl Drop for VideoToolboxH264Encoder {
    fn drop(&mut self) {
        self.tear_down_session();
    }
}

fn configure_session_properties(
    session: NonNull<c_void>,
    config: &H264EncoderConfig,
) -> Result<(), AppRelayError> {
    // SAFETY: every key below is a CoreFoundation static string
    // exported by VideoToolbox; values are core-foundation Rust types
    // converted via TCFType so reference counts are correct.
    unsafe {
        let true_ref = kCFBooleanTrue as CFTypeRef;
        let false_ref = kCFBooleanFalse as CFTypeRef;
        let bitrate_bps = i64::from(config.target_bitrate_kbps).saturating_mul(1_000);

        let bitrate_num = CFNumber::from(bitrate_bps);
        let fps_num = CFNumber::from(i32::try_from(config.max_fps).unwrap_or(i32::MAX));
        let keyframe_interval_num =
            CFNumber::from(i32::try_from(config.keyframe_interval_frames).unwrap_or(i32::MAX));

        set_property(session, kVTCompressionPropertyKey_RealTime, true_ref)?;
        set_property(
            session,
            kVTCompressionPropertyKey_AllowFrameReordering,
            false_ref,
        )?;
        set_property(
            session,
            kVTCompressionPropertyKey_ProfileLevel,
            kVTProfileLevel_H264_Baseline_AutoLevel as CFTypeRef,
        )?;
        set_property(
            session,
            kVTCompressionPropertyKey_AverageBitRate,
            bitrate_num.as_concrete_TypeRef() as CFTypeRef,
        )?;
        set_property(
            session,
            kVTCompressionPropertyKey_ExpectedFrameRate,
            fps_num.as_concrete_TypeRef() as CFTypeRef,
        )?;
        set_property(
            session,
            kVTCompressionPropertyKey_MaxKeyFrameInterval,
            keyframe_interval_num.as_concrete_TypeRef() as CFTypeRef,
        )?;
    }
    Ok(())
}

unsafe fn set_property(
    session: NonNull<c_void>,
    key: CFStringRef,
    value: CFTypeRef,
) -> Result<(), AppRelayError> {
    let status = VTSessionSetProperty(session.as_ptr(), key, value);
    if status != 0 {
        let key_string = if key.is_null() {
            "<null>".to_string()
        } else {
            CFString::wrap_under_get_rule(key).to_string()
        };
        return Err(map_videotoolbox_status(
            &format!("VTSessionSetProperty({key_string})"),
            status,
        ));
    }
    Ok(())
}

unsafe extern "C" fn compression_output_callback(
    output_callback_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: OSStatus,
    _info_flags: VTEncodeInfoFlags,
    sample_buffer: CMSampleBufferRef,
) {
    if output_callback_ref_con.is_null() {
        return;
    }
    // We must NOT consume the strong reference the encoder leaked into
    // the callback context — the encoder owns it for the lifetime of
    // the session. Reconstruct without dropping by incrementing the
    // strong count manually.
    let state_arc = {
        let raw = output_callback_ref_con as *const EncoderState;
        Arc::increment_strong_count(raw);
        Arc::from_raw(raw)
    };

    if status != 0 {
        state_arc
            .sink
            .on_encode_error(map_videotoolbox_status("compression callback", status));
        return;
    }
    if sample_buffer.is_null() {
        return;
    }

    match build_annex_b_payload(&state_arc, sample_buffer) {
        Ok(Some(frame)) => state_arc.sink.on_encoded_frame(frame),
        Ok(None) => {}
        Err(error) => state_arc.sink.on_encode_error(error),
    }
}

unsafe fn build_annex_b_payload(
    state: &EncoderState,
    sample_buffer: CMSampleBufferRef,
) -> Result<Option<EncodedH264Frame>, AppRelayError> {
    let block_buffer = CMSampleBufferGetDataBuffer(sample_buffer);
    if block_buffer.is_null() {
        return Err(AppRelayError::ServiceUnavailable(
            "VideoToolbox returned a sample buffer without a data buffer".to_string(),
        ));
    }

    let total_len = CMBlockBufferGetDataLength(block_buffer);
    if total_len == 0 {
        return Ok(None);
    }

    let mut data = vec![0u8; total_len];
    let copy_status =
        CMBlockBufferCopyDataBytes(block_buffer, 0, total_len, data.as_mut_ptr() as *mut c_void);
    if copy_status != 0 {
        return Err(map_videotoolbox_status(
            "CMBlockBufferCopyDataBytes",
            copy_status,
        ));
    }

    let keyframe = is_sample_buffer_keyframe(sample_buffer);

    // VideoToolbox emits AVCC framing: every NAL unit is prefixed by a
    // 4-byte big-endian length. Translate to Annex-B by replacing each
    // length prefix with `0x00000001`.
    let mut annex_b = Vec::with_capacity(total_len + 16);

    if keyframe {
        let format_desc = CMSampleBufferGetFormatDescription(sample_buffer);
        if !format_desc.is_null() {
            let mut sps_pps = Vec::new();
            extract_h264_parameter_sets(format_desc, &mut sps_pps)?;
            // Cache the latest SPS/PPS so callers can reconstruct the
            // bitstream after a mid-stream subscriber join even though
            // we currently always inline them on keyframes too.
            *state
                .last_sps_pps
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = sps_pps.clone();
            annex_b.extend_from_slice(&sps_pps);
        }
    }

    let mut offset = 0usize;
    while offset + 4 <= data.len() {
        let nal_len = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
        if nal_len == 0 || offset + nal_len > data.len() {
            return Err(AppRelayError::ServiceUnavailable(format!(
                "VideoToolbox produced a malformed AVCC sample buffer (nal_len={nal_len}, offset={offset}, total={})",
                data.len()
            )));
        }
        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(&data[offset..offset + nal_len]);
        offset += nal_len;
    }

    Ok(Some(EncodedH264Frame {
        payload: annex_b,
        keyframe,
        // VideoToolbox does not surface the PTS easily through the
        // sync attachments path; for now we leave it to the caller of
        // encode_pixel_buffer to record the timestamp it submitted and
        // associate it with the matching callback. This is acceptable
        // because the macOS encoder only emits frames in submission
        // order (frame reordering disabled in
        // configure_session_properties).
        timestamp_ms: 0,
    }))
}

unsafe fn is_sample_buffer_keyframe(sample_buffer: CMSampleBufferRef) -> bool {
    let attachments = CMSampleBufferGetSampleAttachmentsArray(sample_buffer, 0);
    if attachments.is_null() {
        // No attachment array means the encoder reported a sync frame.
        return true;
    }
    if CFArrayGetCount(attachments) == 0 {
        return true;
    }
    let dict = CFArrayGetValueAtIndex(attachments, 0) as CFDictionaryRef;
    if dict.is_null() {
        return true;
    }
    let not_sync_value =
        CFDictionaryGetValue(dict, kCMSampleAttachmentKey_NotSync as *const c_void);
    if not_sync_value.is_null() {
        // Key absent → sync frame.
        return true;
    }
    // Key present and equal to kCFBooleanTrue → not a keyframe.
    let not_sync = not_sync_value == kCFBooleanTrue as *const c_void;
    !not_sync
}

unsafe fn extract_h264_parameter_sets(
    format_desc: CMFormatDescriptionRef,
    out: &mut Vec<u8>,
) -> Result<(), AppRelayError> {
    let mut count: usize = 0;
    let mut nal_header_length: i32 = 0;
    // First call: discover the number of parameter sets.
    let status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        format_desc,
        0,
        ptr::null_mut(),
        ptr::null_mut(),
        &mut count,
        &mut nal_header_length,
    );
    if status != 0 {
        return Err(map_videotoolbox_status(
            "CMVideoFormatDescriptionGetH264ParameterSetAtIndex(count)",
            status,
        ));
    }
    if nal_header_length != 4 {
        // We translate AVCC → Annex-B assuming 4-byte length prefixes.
        // A different prefix length means a malformed format.
        return Err(AppRelayError::ServiceUnavailable(format!(
            "VideoToolbox H.264 NAL header length is {nal_header_length}; expected 4"
        )));
    }

    for index in 0..count {
        let mut ptr_out: *const u8 = ptr::null();
        let mut size_out: usize = 0;
        let status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format_desc,
            index,
            &mut ptr_out,
            &mut size_out,
            ptr::null_mut(),
            ptr::null_mut(),
        );
        if status != 0 || ptr_out.is_null() {
            return Err(map_videotoolbox_status(
                "CMVideoFormatDescriptionGetH264ParameterSetAtIndex(set)",
                status,
            ));
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(std::slice::from_raw_parts(ptr_out, size_out));
    }
    Ok(())
}

fn map_videotoolbox_status(context: &str, status: OSStatus) -> AppRelayError {
    // Frequently-seen status codes worth distinguishing in user
    // messages. The full table lives in Apple's `VTErrors.h`; we
    // surface the ones that materially change the user-facing
    // recovery path.
    match status {
        // kVTVideoEncoderNotAvailableNowErr
        -12_915 => AppRelayError::ServiceUnavailable(format!(
            "{context} failed: VideoToolbox encoder is unavailable right now (osstatus {status})"
        )),
        // kVTVideoEncoderMalfunctionErr
        -12_911 => AppRelayError::ServiceUnavailable(format!(
            "{context} failed: VideoToolbox encoder malfunction (osstatus {status})"
        )),
        // kVTCouldNotFindVideoEncoderErr
        -12_908 => AppRelayError::ServiceUnavailable(format!(
            "{context} failed: no H.264 encoder is registered with VideoToolbox (osstatus {status})"
        )),
        // kVTSessionMalfunctionErr / kVTInvalidSessionErr
        -12_903 | -12_902 => AppRelayError::ServiceUnavailable(format!(
            "{context} failed: VideoToolbox session is invalid (osstatus {status})"
        )),
        // kVTPropertyNotSupportedErr / kVTPropertyReadOnlyErr
        -12_900 | -12_901 => AppRelayError::InvalidRequest(format!(
            "{context} failed: VideoToolbox rejected an encoder property (osstatus {status})"
        )),
        // kVTParameterErr (paramErr)
        -50 => AppRelayError::InvalidRequest(format!(
            "{context} failed: VideoToolbox parameter is invalid (osstatus {status})"
        )),
        _ => AppRelayError::ServiceUnavailable(format!(
            "{context} failed: VideoToolbox returned osstatus {status}"
        )),
    }
}

// ------------------------------------------------------------------
// ScreenCaptureKit bridge (active when the macos-screencapturekit
// feature is also enabled). Wraps a `ScreenCaptureKitWindowRuntime`
// and feeds delivered `CMSampleBuffer`s into per-stream
// `VideoToolboxH264Encoder`s. Implements `MacosWindowCaptureRuntime`
// so it can drop straight into `WindowCaptureBackendService`.
// ------------------------------------------------------------------

#[cfg(feature = "macos-screencapturekit")]
use std::collections::HashMap;

#[cfg(feature = "macos-screencapturekit")]
use crate::macos_screencapturekit::{ScreenCaptureKitFrameConsumer, ScreenCaptureKitWindowRuntime};
#[cfg(feature = "macos-screencapturekit")]
use apprelay_protocol::{VideoCaptureRuntimeStatus, ViewportSize};
#[cfg(feature = "macos-screencapturekit")]
use screencapturekit::output::CMSampleBuffer;

// Only the SCK bridge needs to peek inside a sample buffer for the
// underlying CVImageBuffer; declare the import here so default
// `macos-videotoolbox`-only builds (no SCK) do not pull in a dead FFI
// symbol and trigger a `dead_code` warning.
#[cfg(feature = "macos-screencapturekit")]
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetImageBuffer(sample_buffer: CMSampleBufferRef) -> CVImageBufferRef;
}

#[cfg(feature = "macos-screencapturekit")]
struct StreamEncoderEntry {
    /// `Arc` so `BridgeFrameConsumer::on_sample_buffer` can clone a
    /// handle out from under the `encoders` map lock and call
    /// `VTCompressionSessionEncodeFrame` without holding any bridge
    /// mutex across the FFI boundary.
    encoder: Arc<VideoToolboxH264Encoder>,
    target_viewport: ViewportSize,
    last_payload: Arc<Mutex<Vec<u8>>>,
}

#[cfg(feature = "macos-screencapturekit")]
impl std::fmt::Debug for StreamEncoderEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StreamEncoderEntry")
            .field("target_viewport", &self.target_viewport)
            .finish_non_exhaustive()
    }
}

/// `MacosWindowCaptureRuntime` implementation that captures via
/// ScreenCaptureKit and encodes via VideoToolbox. Use this in place
/// of bare `ScreenCaptureKitWindowRuntime` when the
/// `macos-videotoolbox` feature is enabled.
#[cfg(feature = "macos-screencapturekit")]
pub struct VideoToolboxScreenCaptureKitBridge {
    inner: ScreenCaptureKitWindowRuntime,
    state: Arc<BridgeState>,
}

#[cfg(feature = "macos-screencapturekit")]
struct BridgeState {
    encoders: Mutex<HashMap<String, StreamEncoderEntry>>,
    /// Last encoder error observed for each stream, surfaced through
    /// `VideoToolboxScreenCaptureKitBridge::snapshot` so an operator
    /// polling the existing capture-status surface sees encoder
    /// failures instead of a silently empty payload buffer. The map is
    /// cleared when the encoder is reinstalled (start/resize) or torn
    /// down (stop / consumer `on_stream_stopped`).
    encoder_errors: Mutex<HashMap<String, AppRelayError>>,
}

#[cfg(feature = "macos-screencapturekit")]
impl std::fmt::Debug for VideoToolboxScreenCaptureKitBridge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VideoToolboxScreenCaptureKitBridge")
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "macos-screencapturekit")]
impl VideoToolboxScreenCaptureKitBridge {
    pub fn new() -> Self {
        let state = Arc::new(BridgeState {
            encoders: Mutex::new(HashMap::new()),
            encoder_errors: Mutex::new(HashMap::new()),
        });
        let consumer: Arc<dyn ScreenCaptureKitFrameConsumer> = Arc::new(BridgeFrameConsumer {
            state: Arc::clone(&state),
        });
        Self {
            inner: ScreenCaptureKitWindowRuntime::with_frame_consumer(consumer),
            state,
        }
    }

    fn lock_encoders(&self) -> std::sync::MutexGuard<'_, HashMap<String, StreamEncoderEntry>> {
        self.state
            .encoders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Build and configure a fresh per-stream encoder. Invoked on
    /// `start` and on `resize` (resize tears the old encoder down so
    /// the dimensions/bitrate match the new viewport).
    fn install_encoder(
        &self,
        stream_id: &str,
        target_viewport: &ViewportSize,
    ) -> Result<(), AppRelayError> {
        let last_payload = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink: Arc<dyn VideoToolboxEncodedFrameSink> = Arc::new(LatestPayloadSink {
            last_payload: Arc::clone(&last_payload),
        });
        let mut encoder = VideoToolboxH264Encoder::new(sink);
        encoder.configure(H264EncoderConfig {
            width: target_viewport.width,
            height: target_viewport.height,
            // Phase B intentionally does not negotiate these with the
            // existing encoding contract; matching the in-memory
            // pipeline defaults keeps the wiring small.
            target_bitrate_kbps: 2_500,
            max_fps: 30,
            keyframe_interval_frames: 60,
        })?;
        // Reinstalling the encoder clears any stale failure that the
        // previous incarnation surfaced so the operator's poll path
        // does not show an error against a freshly-built session.
        self.state
            .encoder_errors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(stream_id);
        self.lock_encoders().insert(
            stream_id.to_string(),
            StreamEncoderEntry {
                encoder: Arc::new(encoder),
                target_viewport: target_viewport.clone(),
                last_payload,
            },
        );
        Ok(())
    }

    fn remove_encoder(&self, stream_id: &str) {
        // Drop the entry so the inner `VideoToolboxH264Encoder`'s `Drop`
        // impl tears the `VTCompressionSession` down. Any other live
        // `Arc` clone (e.g. one momentarily held by an in-flight
        // `BridgeFrameConsumer::on_sample_buffer` call) will run the
        // tear-down once it goes out of scope.
        self.lock_encoders().remove(stream_id);
        self.state
            .encoder_errors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(stream_id);
    }
}

#[cfg(feature = "macos-screencapturekit")]
impl Default for VideoToolboxScreenCaptureKitBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "macos-screencapturekit")]
impl MacosWindowCaptureRuntime for VideoToolboxScreenCaptureKitBridge {
    fn start(&self, request: MacosWindowCaptureStartRequest) -> Result<(), AppRelayError> {
        // Bring the encoder up first so the very first delivered
        // sample buffer can be encoded; if the SCK start fails below,
        // the encoder is torn down again.
        self.install_encoder(&request.stream_id, &request.target_viewport)?;
        if let Err(error) = self.inner.start(request.clone()) {
            self.remove_encoder(&request.stream_id);
            return Err(error);
        }
        Ok(())
    }

    fn resize(&self, request: MacosWindowCaptureResizeRequest) -> Result<(), AppRelayError> {
        self.inner.resize(request.clone())?;
        // Rebuild the encoder so the compression session matches the
        // new dimensions. Failure here leaves the SCK side running so
        // the caller can decide whether to stop the stream.
        self.install_encoder(&request.stream_id, &request.target_viewport)?;
        Ok(())
    }

    fn stop(&self, stream_id: &str) {
        self.inner.stop(stream_id);
        self.remove_encoder(stream_id);
    }

    fn snapshot(&self, stream_id: &str) -> Option<VideoCaptureRuntimeStatus> {
        // If the VideoToolbox encoder has failed for this stream,
        // surface that on the existing capture-status surface so an
        // operator polling `snapshot` is not misled by a healthy SCK
        // delivery state behind a broken encoder. The encoder error
        // takes precedence because, from the operator's perspective,
        // the pipeline is no longer producing usable frames.
        let encoder_error = self
            .state
            .encoder_errors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(stream_id)
            .cloned();
        if let Some(error) = encoder_error {
            let state = match &error {
                AppRelayError::PermissionDenied(_) => {
                    apprelay_protocol::VideoCaptureRuntimeState::PermissionDenied
                }
                _ => apprelay_protocol::VideoCaptureRuntimeState::Failed,
            };
            // Preserve the SCK delivery counters if we have them; only
            // override state/message so callers still see the last
            // delivered-frame metadata that ScreenCaptureKit recorded
            // before the encoder broke.
            let base = self.inner.snapshot(stream_id).unwrap_or_default();
            return Some(VideoCaptureRuntimeStatus {
                state,
                frames_delivered: base.frames_delivered,
                last_frame: base.last_frame,
                message: Some(error.user_message()),
            });
        }
        self.inner.snapshot(stream_id)
    }

    fn latest_encoded_payload(&self, stream_id: &str) -> Option<Vec<u8>> {
        let guard = self.lock_encoders();
        let entry = guard.get(stream_id)?;
        let payload = entry
            .last_payload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if payload.is_empty() {
            None
        } else {
            Some(payload)
        }
    }
}

#[cfg(feature = "macos-screencapturekit")]
struct BridgeFrameConsumer {
    state: Arc<BridgeState>,
}

#[cfg(feature = "macos-screencapturekit")]
impl ScreenCaptureKitFrameConsumer for BridgeFrameConsumer {
    fn on_sample_buffer(&self, stream_id: &str, sample_buffer: &CMSampleBuffer) {
        // Snapshot the per-stream encoder handle under the lock and
        // drop the guard *before* we cross the VideoToolbox FFI
        // boundary. Holding `encoders` across
        // `VTCompressionSessionEncodeFrame` could stall a concurrent
        // `stop`/`resize`/`latest_encoded_payload` call (which all
        // re-enter `lock_encoders`) and risks deadlocking
        // ScreenCaptureKit shutdown if `SCStream::stop_capture`
        // synchronises with this delivery thread.
        let encoder = {
            let guard = self
                .state
                .encoders
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(entry) = guard.get(stream_id) else {
                return;
            };
            Arc::clone(&entry.encoder)
        };

        // The screencapturekit crate hands us a typed CMSampleBuffer.
        // Reach into it via FFI for the underlying `CVImageBuffer`
        // pointer; the typed wrapper does not expose this directly.
        // SAFETY: as_concrete_TypeRef-style accessors on the wrapper
        // return a non-null reference for as long as the wrapper is
        // alive, which is the duration of this callback.
        let sample_buffer_ref = sample_buffer_ptr(sample_buffer);
        if sample_buffer_ref.is_null() {
            return;
        }
        let pixel_buffer = unsafe { CMSampleBufferGetImageBuffer(sample_buffer_ref) };
        if pixel_buffer.is_null() {
            return;
        }
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or_default();
        // SAFETY: pixel_buffer was just retrieved from a live sample
        // buffer in this callback frame and is non-null. The encoder
        // handle is an `Arc` clone taken under (and released before
        // entering) the FFI call, so no bridge mutex is held here.
        if let Err(error) = unsafe { encoder.encode_pixel_buffer(pixel_buffer, timestamp_ms) } {
            // Surface the encoder failure on the per-stream snapshot
            // path so the operator polling
            // `VideoToolboxScreenCaptureKitBridge::snapshot` sees a
            // typed `Failed`/`PermissionDenied` state instead of a
            // silently empty payload. We deliberately do not panic
            // here because the SCK delivery thread is shared and a
            // single bad frame must not tear the capture stream down.
            self.state
                .encoder_errors
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(stream_id.to_string(), error);
        }
    }

    fn on_stream_stopped(&self, stream_id: &str) {
        // Drop the entry; `VideoToolboxH264Encoder::Drop` invalidates
        // the underlying `VTCompressionSession`. If a delivery-thread
        // `on_sample_buffer` is mid-encode for this stream it still
        // owns an `Arc` clone; the session is torn down when the last
        // clone is dropped.
        self.state
            .encoders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(stream_id);
        self.state
            .encoder_errors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(stream_id);
    }
}

#[cfg(feature = "macos-screencapturekit")]
struct LatestPayloadSink {
    last_payload: Arc<Mutex<Vec<u8>>>,
}

#[cfg(feature = "macos-screencapturekit")]
impl VideoToolboxEncodedFrameSink for LatestPayloadSink {
    fn on_encoded_frame(&self, frame: EncodedH264Frame) {
        if frame.payload.is_empty() {
            return;
        }
        *self
            .last_payload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = frame.payload;
    }

    fn on_encode_error(&self, _error: AppRelayError) {
        // Errors are surfaced via the SCK runtime's snapshot/health
        // channel rather than the payload buffer.
    }
}

/// Best-effort accessor for the underlying `CMSampleBufferRef` of a
/// `screencapturekit::output::CMSampleBuffer`. The wrapper exposes
/// `as_concrete_TypeRef()` via `core_foundation::base::TCFType`, which
/// returns the raw CoreFoundation handle.
#[cfg(feature = "macos-screencapturekit")]
fn sample_buffer_ptr(sample_buffer: &CMSampleBuffer) -> CMSampleBufferRef {
    use core_foundation::base::TCFType;
    sample_buffer.as_concrete_TypeRef() as CMSampleBufferRef
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_status_classifies_invalid_session_as_service_unavailable() {
        match map_videotoolbox_status("ctx", -12_903) {
            AppRelayError::ServiceUnavailable(msg) => assert!(msg.contains("invalid")),
            other => panic!("expected ServiceUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn map_status_classifies_param_err_as_invalid_request() {
        match map_videotoolbox_status("ctx", -50) {
            AppRelayError::InvalidRequest(msg) => assert!(msg.contains("parameter")),
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
    }

    #[test]
    fn map_status_unknown_codes_fall_back_to_service_unavailable() {
        match map_videotoolbox_status("ctx", -1) {
            AppRelayError::ServiceUnavailable(msg) => assert!(msg.contains("osstatus -1")),
            other => panic!("expected ServiceUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn cm_time_helpers_are_marked_valid() {
        let t = cm_time(123, 1_000);
        assert_eq!(t.value, 123);
        assert_eq!(t.timescale, 1_000);
        assert_eq!(t.flags, K_CMTIME_FLAG_VALID);
        let invalid = cm_time_invalid();
        assert_eq!(invalid.flags, 0);
    }

    /// Configure with a known-bad config and confirm the framework
    /// rejection is surfaced as a typed error, not a panic.
    #[test]
    fn configure_rejects_zero_dimensions_before_touching_videotoolbox() {
        struct DropSink;
        impl VideoToolboxEncodedFrameSink for DropSink {
            fn on_encoded_frame(&self, _frame: EncodedH264Frame) {}
            fn on_encode_error(&self, _error: AppRelayError) {}
        }
        let mut encoder = VideoToolboxH264Encoder::new(Arc::new(DropSink));
        let err = encoder
            .configure(H264EncoderConfig {
                width: 0,
                height: 720,
                target_bitrate_kbps: 1_000,
                max_fps: 30,
                keyframe_interval_frames: 60,
            })
            .expect_err("zero width must be rejected");
        assert!(matches!(err, AppRelayError::InvalidRequest(_)));
    }

    #[test]
    fn encode_without_configure_returns_service_unavailable() {
        struct DropSink;
        impl VideoToolboxEncodedFrameSink for DropSink {
            fn on_encoded_frame(&self, _frame: EncodedH264Frame) {}
            fn on_encode_error(&self, _error: AppRelayError) {}
        }
        let encoder = VideoToolboxH264Encoder::new(Arc::new(DropSink));
        // SAFETY: passing a dummy non-null pointer is fine because the
        // function returns before dereferencing it (it bails on the
        // missing session first).
        let dummy = NonNull::<c_void>::dangling().as_ptr();
        let err =
            unsafe { encoder.encode_pixel_buffer(dummy, 0) }.expect_err("must require configure");
        assert!(matches!(err, AppRelayError::ServiceUnavailable(_)));
    }

    /// Fix 1: when a `BridgeFrameConsumer` records an encoder error
    /// for a stream, the bridge's `snapshot()` (the operator's
    /// existing capture-status surface) must report
    /// `Failed`/`PermissionDenied` with the error's user message
    /// instead of silently masking it.
    #[cfg(feature = "macos-screencapturekit")]
    #[test]
    fn snapshot_surfaces_recorded_encoder_error_as_failed() {
        let bridge = VideoToolboxScreenCaptureKitBridge::new();
        bridge
            .state
            .encoder_errors
            .lock()
            .expect("lock encoder_errors")
            .insert(
                "stream-err".to_string(),
                AppRelayError::ServiceUnavailable("encoder went sideways".to_string()),
            );
        let snapshot = bridge
            .snapshot("stream-err")
            .expect("encoder error must produce a snapshot even if SCK never recorded one");
        assert_eq!(
            snapshot.state,
            apprelay_protocol::VideoCaptureRuntimeState::Failed
        );
        let message = snapshot
            .message
            .expect("encoder error message must surface");
        assert!(
            message.contains("encoder went sideways"),
            "expected user_message to be carried through, got {message:?}"
        );
    }

    #[cfg(feature = "macos-screencapturekit")]
    #[test]
    fn snapshot_maps_permission_denied_encoder_error() {
        let bridge = VideoToolboxScreenCaptureKitBridge::new();
        bridge
            .state
            .encoder_errors
            .lock()
            .expect("lock encoder_errors")
            .insert(
                "stream-perm".to_string(),
                AppRelayError::PermissionDenied("screen recording denied".to_string()),
            );
        let snapshot = bridge.snapshot("stream-perm").expect("snapshot present");
        assert_eq!(
            snapshot.state,
            apprelay_protocol::VideoCaptureRuntimeState::PermissionDenied
        );
    }

    /// Fix 1: tearing the encoder down must clear its recorded error
    /// so a subsequent reinstall is not haunted by stale failure
    /// state. (`remove_encoder` is the shared cleanup path used by
    /// stop and on-stream-stopped.)
    #[cfg(feature = "macos-screencapturekit")]
    #[test]
    fn remove_encoder_clears_recorded_encoder_error() {
        let bridge = VideoToolboxScreenCaptureKitBridge::new();
        bridge
            .state
            .encoder_errors
            .lock()
            .expect("lock encoder_errors")
            .insert(
                "stream-clear".to_string(),
                AppRelayError::ServiceUnavailable("stale".to_string()),
            );
        bridge.remove_encoder("stream-clear");
        assert!(
            !bridge
                .state
                .encoder_errors
                .lock()
                .expect("lock encoder_errors")
                .contains_key("stream-clear"),
            "remove_encoder must drop the stale encoder_errors entry"
        );
    }
}
