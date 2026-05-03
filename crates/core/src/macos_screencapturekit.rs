//! ScreenCaptureKit-backed selected-window capture runtime for macOS.
//!
//! This module owns the macOS-only capture runtime that replaces
//! `ControlPlaneMacosWindowCaptureRuntime` once the cargo feature
//! `macos-screencapturekit` is enabled. It uses the high-level
//! [`screencapturekit`] crate (svtlabs) which wraps Apple's
//! `ScreenCaptureKit.framework`. The high-level wrapper was chosen over
//! direct `objc2-screen-capture-kit` calls because implementing the
//! `SCStreamOutput`/`SCStreamDelegate` Objective-C protocols by hand is
//! involved, and the wrapper keeps the unsafe surface inside a single
//! audited dependency rather than spreading it across this crate.
//!
//! Responsibilities:
//! - Locate the macOS native `SCWindow` whose `windowID` matches the
//!   numeric component of the AppRelay `selected_window_id` (format
//!   `macos-window-{session_id}-{native_id}`, established in
//!   `crates/core/src/lib.rs`).
//! - Start an `SCStream` filtered to that single window with a
//!   configuration sized to the requested viewport.
//! - Maintain a per-`stream_id` `VideoCaptureRuntimeStatus` snapshot the
//!   server can poll. Each delivered frame increments `frames_delivered`
//!   and refreshes `last_frame`.
//! - Surface every error path as a typed `AppRelayError`, never silently
//!   no-op. Permission denials map to `AppRelayError::PermissionDenied`,
//!   missing windows to `AppRelayError::NotFound`, framework failures to
//!   `AppRelayError::ServiceUnavailable`.
//!
//! Caveat: the AppRelay `selected_window_id` is sourced from
//! `System Events` via AppleScript today, which exposes the
//! Accessibility window id. For most macOS applications that id matches
//! the `CGWindowID` ScreenCaptureKit reports, but for some apps it does
//! not. When the ids do not align, `start` returns
//! `AppRelayError::NotFound` with a message naming the missing
//! CGWindowID. Tightening that mapping is tracked separately and is not
//! in scope for Phase A.1.

#![cfg(all(feature = "macos-screencapturekit", target_os = "macos"))]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use apprelay_protocol::{
    AppRelayError, CapturedVideoFrame, VideoCaptureRuntimeState, VideoCaptureRuntimeStatus,
    ViewportSize,
};
use core_foundation::error::CFError;
use screencapturekit::{
    output::{CMSampleBuffer, CVPixelBuffer},
    shareable_content::{window::SCWindow, SCShareableContent},
    stream::{
        configuration::SCStreamConfiguration, content_filter::SCContentFilter,
        output_trait::SCStreamOutputTrait, output_type::SCStreamOutputType, SCStream,
    },
};

use crate::video_stream::{
    MacosWindowCaptureResizeRequest, MacosWindowCaptureRuntime, MacosWindowCaptureStartRequest,
};

#[derive(Debug, Default)]
pub struct ScreenCaptureKitWindowRuntime {
    snapshots: Arc<Mutex<HashMap<String, VideoCaptureRuntimeStatus>>>,
    streams: Arc<Mutex<HashMap<String, ActiveStream>>>,
}

impl ScreenCaptureKitWindowRuntime {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Tracks an in-flight `SCStream` so we can stop it later. The stream is
/// kept alive while it is in this map; dropping it tears the underlying
/// objc2 stream down via the wrapper's `Drop` implementation.
struct ActiveStream {
    stream: SCStream,
    selected_window_id: String,
    application_id: String,
    title: String,
    target_viewport: ViewportSize,
    /// Native CGWindowID parsed from `selected_window_id`. Held so a
    /// `resize` does not have to re-parse it.
    native_window_id: u32,
}

impl std::fmt::Debug for ActiveStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActiveStream")
            .field("selected_window_id", &self.selected_window_id)
            .field("application_id", &self.application_id)
            .field("title", &self.title)
            .field("target_viewport", &self.target_viewport)
            .field("native_window_id", &self.native_window_id)
            .finish_non_exhaustive()
    }
}

impl MacosWindowCaptureRuntime for ScreenCaptureKitWindowRuntime {
    fn start(&self, request: MacosWindowCaptureStartRequest) -> Result<(), AppRelayError> {
        // Refuse double-start so the snapshot map and the stream map
        // never disagree about ownership.
        if self.lock_streams().contains_key(&request.stream_id) {
            return Err(AppRelayError::InvalidRequest(format!(
                "ScreenCaptureKit stream {} is already running",
                request.stream_id
            )));
        }

        // Mark Starting up front so callers polling `snapshot` immediately
        // see the runtime is engaged, even before the first frame lands.
        self.write_snapshot(
            &request.stream_id,
            VideoCaptureRuntimeStatus {
                state: VideoCaptureRuntimeState::Starting,
                frames_delivered: 0,
                last_frame: None,
                message: None,
            },
        );

        let native_window_id = match parse_native_window_id(&request.selected_window_id) {
            Ok(id) => id,
            Err(error) => {
                self.write_snapshot(
                    &request.stream_id,
                    failure_snapshot_for(&error, "ScreenCaptureKit window id is invalid"),
                );
                return Err(error);
            }
        };
        let target_viewport = request.target_viewport.clone();

        match self.start_stream(&request, native_window_id, &target_viewport) {
            Ok(stream) => {
                self.lock_streams().insert(
                    request.stream_id.clone(),
                    ActiveStream {
                        stream,
                        selected_window_id: request.selected_window_id,
                        application_id: request.application_id,
                        title: request.title,
                        target_viewport,
                        native_window_id,
                    },
                );
                Ok(())
            }
            Err(error) => {
                self.write_snapshot(
                    &request.stream_id,
                    failure_snapshot_for(&error, "ScreenCaptureKit failed to start capture"),
                );
                Err(error)
            }
        }
    }

    fn resize(&self, request: MacosWindowCaptureResizeRequest) -> Result<(), AppRelayError> {
        // ScreenCaptureKit's high-level Rust wrapper does not expose
        // `updateConfiguration:completionHandler:`. Honour the new
        // viewport by stopping the old SCStream, building a fresh one
        // with the new dimensions, and swapping it in.
        let mut streams = self.lock_streams();
        let Some(active) = streams.remove(&request.stream_id) else {
            return Err(AppRelayError::NotFound(format!(
                "ScreenCaptureKit stream {} is not running",
                request.stream_id
            )));
        };

        if active.selected_window_id != request.selected_window_id {
            let message = format!(
                "stream {} captures window {} but resize was requested for {}",
                request.stream_id, active.selected_window_id, request.selected_window_id
            );
            // Put the original stream back so the resize rejection does
            // not also drop the live capture.
            streams.insert(request.stream_id.clone(), active);
            return Err(AppRelayError::InvalidRequest(message));
        }

        if let Err(stop_err) = active.stream.stop_capture() {
            // Best-effort: even if the old stream refused to stop, drop
            // it and try to spin up the replacement so the caller is not
            // left in a frozen state. The original error is reported.
            drop(active);
            let app_err = map_cf_error(
                "ScreenCaptureKit stop_capture failed during resize",
                &stop_err,
            );
            self.write_snapshot(
                &request.stream_id,
                failure_snapshot_for(
                    &app_err,
                    "ScreenCaptureKit stop_capture failed during resize",
                ),
            );
            return Err(app_err);
        }

        let start_request = MacosWindowCaptureStartRequest {
            stream_id: request.stream_id.clone(),
            selected_window_id: request.selected_window_id.clone(),
            application_id: active.application_id.clone(),
            title: active.title.clone(),
            target_viewport: request.target_viewport.clone(),
        };
        let target_viewport = request.target_viewport.clone();

        match self.start_stream(&start_request, active.native_window_id, &target_viewport) {
            Ok(new_stream) => {
                streams.insert(
                    request.stream_id.clone(),
                    ActiveStream {
                        stream: new_stream,
                        selected_window_id: request.selected_window_id,
                        application_id: active.application_id,
                        title: active.title,
                        target_viewport,
                        native_window_id: active.native_window_id,
                    },
                );
                Ok(())
            }
            Err(error) => {
                self.write_snapshot(
                    &request.stream_id,
                    failure_snapshot_for(
                        &error,
                        "ScreenCaptureKit failed to restart capture after resize",
                    ),
                );
                Err(error)
            }
        }
    }

    fn stop(&self, stream_id: &str) {
        if let Some(active) = self.lock_streams().remove(stream_id) {
            // Best-effort: surface any stop failure into the snapshot so
            // observers see the runtime didn't shut down cleanly. We
            // still drop the stream either way.
            if let Err(error) = active.stream.stop_capture() {
                let app_error = map_cf_error("ScreenCaptureKit stop_capture failed", &error);
                let previous = self.snapshot(stream_id).unwrap_or_default();
                self.write_snapshot(
                    stream_id,
                    VideoCaptureRuntimeStatus {
                        state: VideoCaptureRuntimeState::Failed,
                        frames_delivered: previous.frames_delivered,
                        last_frame: previous.last_frame,
                        message: Some(app_error.user_message()),
                    },
                );
            }
        }
        self.lock_snapshots().remove(stream_id);
    }

    fn snapshot(&self, stream_id: &str) -> Option<VideoCaptureRuntimeStatus> {
        self.lock_snapshots().get(stream_id).cloned()
    }
}

impl ScreenCaptureKitWindowRuntime {
    fn lock_snapshots(&self) -> MutexGuard<'_, HashMap<String, VideoCaptureRuntimeStatus>> {
        self.snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_streams(&self) -> MutexGuard<'_, HashMap<String, ActiveStream>> {
        self.streams
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_snapshot(&self, stream_id: &str, snapshot: VideoCaptureRuntimeStatus) {
        self.lock_snapshots()
            .insert(stream_id.to_string(), snapshot);
    }

    /// Build, configure, and start a fresh `SCStream` filtered to the
    /// single requested window. The returned stream is owned by the
    /// caller and must be retained for the lifetime of the capture.
    fn start_stream(
        &self,
        request: &MacosWindowCaptureStartRequest,
        native_window_id: u32,
        target_viewport: &ViewportSize,
    ) -> Result<SCStream, AppRelayError> {
        let window = lookup_window(native_window_id)?;
        let filter = SCContentFilter::new().with_desktop_independent_window(&window);
        let configuration = build_configuration(target_viewport)?;

        let mut stream = SCStream::new(&filter, &configuration);
        let handler = FrameSink {
            stream_id: request.stream_id.clone(),
            snapshots: Arc::clone(&self.snapshots),
            target_viewport: target_viewport.clone(),
        };
        stream.add_output_handler(handler, SCStreamOutputType::Screen);

        stream
            .start_capture()
            .map_err(|error| map_cf_error("ScreenCaptureKit start_capture failed", &error))?;

        Ok(stream)
    }
}

/// Per-stream output handler. Updates the shared snapshot map every
/// time ScreenCaptureKit hands us a frame.
struct FrameSink {
    stream_id: String,
    snapshots: Arc<Mutex<HashMap<String, VideoCaptureRuntimeStatus>>>,
    target_viewport: ViewportSize,
}

impl SCStreamOutputTrait for FrameSink {
    fn did_output_sample_buffer(&self, sample_buffer: CMSampleBuffer, of_type: SCStreamOutputType) {
        if !matches!(of_type, SCStreamOutputType::Screen) {
            return;
        }

        let size =
            pixel_buffer_size(&sample_buffer).unwrap_or_else(|| self.target_viewport.clone());
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or_default();

        let mut snapshots = self
            .snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = snapshots.get(&self.stream_id).cloned().unwrap_or_default();
        let sequence = previous.frames_delivered.saturating_add(1);
        snapshots.insert(
            self.stream_id.clone(),
            VideoCaptureRuntimeStatus {
                state: VideoCaptureRuntimeState::Delivering,
                frames_delivered: sequence,
                last_frame: Some(CapturedVideoFrame {
                    sequence,
                    timestamp_ms,
                    size,
                }),
                message: None,
            },
        );
    }
}

fn pixel_buffer_size(sample_buffer: &CMSampleBuffer) -> Option<ViewportSize> {
    let pixel_buffer: CVPixelBuffer = sample_buffer.get_pixel_buffer().ok()?;
    Some(ViewportSize {
        width: pixel_buffer.get_width(),
        height: pixel_buffer.get_height(),
    })
}

fn build_configuration(
    target_viewport: &ViewportSize,
) -> Result<SCStreamConfiguration, AppRelayError> {
    SCStreamConfiguration::new()
        .set_width(target_viewport.width)
        .and_then(|config| config.set_height(target_viewport.height))
        .and_then(|config| config.set_shows_cursor(false))
        .map_err(|error| map_cf_error("ScreenCaptureKit configuration setup failed", &error))
}

/// Locate the `SCWindow` whose CGWindowID matches the requested id.
fn lookup_window(native_window_id: u32) -> Result<SCWindow, AppRelayError> {
    let content = SCShareableContent::get().map_err(|error| {
        map_cf_error(
            "failed to enumerate ScreenCaptureKit shareable content (Screen Recording permission may be missing)",
            &error,
        )
    })?;

    content
        .windows()
        .into_iter()
        .find(|window| window.window_id() == native_window_id)
        .ok_or_else(|| {
            AppRelayError::NotFound(format!(
                "ScreenCaptureKit could not find a shareable window with CGWindowID {native_window_id} (the window may have closed, be on a different Space, or use a different id than the AppRelay window selector reports)"
            ))
        })
}

/// Selected window ids produced by `select_macos_native_window` have the
/// shape `macos-window-{session_id}-{native_id}`. Re-parse just the
/// trailing native id and validate it as a `u32`.
fn parse_native_window_id(selected_window_id: &str) -> Result<u32, AppRelayError> {
    let encoded = selected_window_id
        .strip_prefix("macos-window-")
        .ok_or_else(|| {
            AppRelayError::InvalidRequest(format!(
                "selected window id `{selected_window_id}` is not a macOS native window id"
            ))
        })?;
    let (_, native) = encoded.rsplit_once('-').ok_or_else(|| {
        AppRelayError::InvalidRequest(format!(
            "selected window id `{selected_window_id}` is missing a macOS native window id"
        ))
    })?;
    native.trim().parse::<u32>().map_err(|_| {
        AppRelayError::InvalidRequest(format!(
            "selected window id `{selected_window_id}` has an unusable macOS native window id"
        ))
    })
}

fn map_cf_error(context: &str, error: &CFError) -> AppRelayError {
    let formatted = format!("{context}: {error}");
    classify_error_message(formatted)
}

fn classify_error_message(message: String) -> AppRelayError {
    let lower = message.to_lowercase();
    if lower.contains("permission")
        || lower.contains("tcc")
        || lower.contains("not authorized")
        || lower.contains("declined")
    {
        AppRelayError::PermissionDenied(message)
    } else {
        AppRelayError::ServiceUnavailable(message)
    }
}

fn failure_snapshot_for(error: &AppRelayError, _context: &str) -> VideoCaptureRuntimeStatus {
    let state = match error {
        AppRelayError::PermissionDenied(_) => VideoCaptureRuntimeState::PermissionDenied,
        _ => VideoCaptureRuntimeState::Failed,
    };
    VideoCaptureRuntimeStatus {
        state,
        frames_delivered: 0,
        last_frame: None,
        message: Some(error.user_message()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_native_window_id_round_trips_session_format() {
        let id = parse_native_window_id("macos-window-session-42-12345").expect("parses");
        assert_eq!(id, 12_345);
    }

    #[test]
    fn parse_native_window_id_rejects_unprefixed_input() {
        let err = parse_native_window_id("12345").expect_err("expected invalid request");
        assert!(matches!(err, AppRelayError::InvalidRequest(_)));
    }

    #[test]
    fn parse_native_window_id_rejects_non_numeric_native_id() {
        let err = parse_native_window_id("macos-window-session-abc")
            .expect_err("expected invalid request");
        assert!(matches!(err, AppRelayError::InvalidRequest(_)));
    }

    #[test]
    fn snapshot_is_empty_until_capture_starts() {
        let runtime = ScreenCaptureKitWindowRuntime::new();
        assert!(runtime.snapshot("stream-1").is_none());
    }

    #[test]
    fn stop_on_unknown_stream_is_a_noop() {
        let runtime = ScreenCaptureKitWindowRuntime::new();
        runtime.stop("stream-1");
        assert!(runtime.snapshot("stream-1").is_none());
    }

    #[test]
    fn start_with_invalid_window_id_format_returns_typed_error() {
        let runtime = ScreenCaptureKitWindowRuntime::new();
        let err = runtime
            .start(MacosWindowCaptureStartRequest {
                stream_id: "stream-1".into(),
                selected_window_id: "not-a-macos-id".into(),
                application_id: "app".into(),
                title: "T".into(),
                target_viewport: ViewportSize {
                    width: 320,
                    height: 240,
                },
            })
            .expect_err("invalid id should fail");
        assert!(matches!(err, AppRelayError::InvalidRequest(_)));
        // Snapshot must reflect the failure so observers do not see a
        // silent no-op.
        let snapshot = runtime.snapshot("stream-1").expect("snapshot recorded");
        assert_eq!(snapshot.state, VideoCaptureRuntimeState::Failed);
        assert!(snapshot.message.is_some());
    }

    #[test]
    fn classify_permission_messages_as_permission_denied() {
        let denied = classify_error_message("permission denied by TCC".to_string());
        assert!(matches!(denied, AppRelayError::PermissionDenied(_)));
        let unavailable = classify_error_message("framework returned -3801".to_string());
        assert!(matches!(unavailable, AppRelayError::ServiceUnavailable(_)));
    }
}
