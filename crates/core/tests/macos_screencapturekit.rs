//! Integration test for `ScreenCaptureKitWindowRuntime`.
//!
//! This test exercises the real `ScreenCaptureKit.framework` capture
//! pipeline end-to-end: it starts an `SCStream` against a live shareable
//! window, waits for ScreenCaptureKit to deliver a few frames, asserts
//! that `snapshot` reflects them, then stops cleanly.
//!
//! It is `#[ignore]` because it requires:
//!   * macOS host (compile-time gated)
//!   * the `macos-screencapturekit` cargo feature (compile-time gated)
//!   * Screen Recording permission granted to the test runner under
//!     System Settings -> Privacy & Security
//!   * at least one shareable on-screen window
//!
//! Run locally with:
//!   cargo test -p apprelay-core --features macos-screencapturekit \
//!     --test macos_screencapturekit -- --ignored

#![cfg(all(feature = "macos-screencapturekit", target_os = "macos"))]

use std::thread;
use std::time::{Duration, Instant};

use apprelay_core::{
    MacosWindowCaptureRuntime, MacosWindowCaptureStartRequest, ScreenCaptureKitWindowRuntime,
};
use apprelay_protocol::{VideoCaptureRuntimeState, ViewportSize};
use screencapturekit::shareable_content::SCShareableContent;

#[test]
#[ignore = "requires Screen Recording permission and a live shareable window"]
fn screencapturekit_runtime_delivers_real_frames() {
    let content = SCShareableContent::get()
        .expect("Screen Recording permission must be granted; SCShareableContent::get failed");

    let window = content
        .windows()
        .into_iter()
        .find(|window| window.is_on_screen() && !window.title().is_empty())
        .expect("at least one on-screen titled shareable window must exist");

    let native_id = window.window_id();
    let stream_id = "integration-stream";
    let selected_window_id = format!("macos-window-integration-session-{native_id}");

    let runtime = ScreenCaptureKitWindowRuntime::new();
    runtime
        .start(MacosWindowCaptureStartRequest {
            stream_id: stream_id.into(),
            selected_window_id: selected_window_id.clone(),
            application_id: "integration.app".into(),
            title: window.title(),
            target_viewport: ViewportSize {
                width: 640,
                height: 480,
            },
        })
        .expect("ScreenCaptureKitWindowRuntime::start should succeed");

    // Poll until at least three frames are delivered or we time out.
    // ScreenCaptureKit needs a few hundred milliseconds to spin up
    // capture; budget five seconds so transient hiccups do not flake.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_snapshot = None;
    while Instant::now() < deadline {
        let snapshot = runtime
            .snapshot(stream_id)
            .expect("snapshot should exist while the stream is active");
        if snapshot.frames_delivered >= 3 {
            last_snapshot = Some(snapshot);
            break;
        }
        last_snapshot = Some(snapshot);
        thread::sleep(Duration::from_millis(100));
    }

    runtime.stop(stream_id);

    let snapshot = last_snapshot.expect("polling loop must observe at least one snapshot");
    assert!(
        snapshot.frames_delivered >= 3,
        "expected at least 3 frames within deadline, got {} (state={:?}, message={:?})",
        snapshot.frames_delivered,
        snapshot.state,
        snapshot.message,
    );
    assert_eq!(snapshot.state, VideoCaptureRuntimeState::Delivering);
    let frame = snapshot
        .last_frame
        .expect("a delivering snapshot must carry a last_frame");
    assert_eq!(frame.sequence, snapshot.frames_delivered);
    assert!(
        frame.size.width > 0 && frame.size.height > 0,
        "captured frame must have non-zero dimensions, got {:?}",
        frame.size,
    );

    // After stop, the snapshot map is cleared.
    assert!(runtime.snapshot(stream_id).is_none());
}
