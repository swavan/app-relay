import { describe, expect, it } from "vitest";
import { buildVideoRendererView } from "./videoRenderer";
import type { VideoStreamSession } from "./videoStreams";

const baseStream: VideoStreamSession = {
  id: "stream-1",
  sessionId: "session-1",
  selectedWindowId: "window-session-1",
  viewport: {
    width: 2560,
    height: 1440,
  },
  captureSource: {
    scope: "selectedWindow",
    selectedWindowId: "window-session-1",
    applicationId: "terminal",
    title: "Terminal",
  },
  encoding: {
    contract: {
      codec: "h264",
      pixelFormat: "rgba",
      hardwareAcceleration: "none",
      target: {
        resolution: {
          width: 1920,
          height: 1080,
        },
        maxFps: 30,
        targetBitrateKbps: 4500,
        keyframeIntervalFrames: 60,
      },
      adaptation: {
        requestedViewport: {
          width: 2560,
          height: 1440,
        },
        currentTarget: {
          width: 1920,
          height: 1080,
        },
        limits: {
          maxWidth: 1920,
          maxHeight: 1080,
          maxPixels: 2073600,
        },
        reason: "cappedToLimits",
      },
    },
    state: "encoding",
    output: {
      framesSubmitted: 3,
      framesEncoded: 3,
      keyframesEncoded: 1,
      bytesProduced: 1536,
      lastFrame: {
        sequence: 3,
        timestampMs: 99,
        byteLength: 512,
        keyframe: false,
      },
    },
  },
  signaling: {
    kind: "webRtcOffer",
    negotiationState: "negotiated",
    iceCandidates: [
      {
        candidate: "candidate:apprelay stream-1 window-session-1 typ host",
      },
    ],
  },
  stats: {
    framesEncoded: 3,
    bitrateKbps: 4500,
    latencyMs: 33,
    reconnectAttempts: 1,
  },
  health: {
    healthy: true,
    message: "stream negotiated",
  },
  state: "streaming",
};

describe("buildVideoRendererView", () => {
  it("returns an empty renderer state before stream start", () => {
    const view = buildVideoRendererView(null, { width: 1280, height: 720 });

    expect(view.state).toBe("empty");
    expect(view.stateLabel).toBe("No stream");
    expect(view.requestedResolutionLabel).toBe("1280 x 720");
    expect(view.targetResolutionLabel).toBe("No encoding target");
    expect(view.hasEncodedFrame).toBe(false);
  });

  it("summarizes selected-window and encoded-frame metadata", () => {
    const view = buildVideoRendererView(baseStream, { width: 2560, height: 1440 });

    expect(view.state).toBe("streaming");
    expect(view.windowTitle).toBe("Terminal");
    expect(view.requestedResolutionLabel).toBe("2560 x 1440");
    expect(view.targetResolutionLabel).toBe("1920 x 1080");
    expect(view.viewportResolutionLabel).toBe("2560 x 1440");
    expect(view.adaptationLabel).toBe("Target capped to encoder limits");
    expect(view.framesLabel).toBe("3 encoded");
    expect(view.keyframesLabel).toBe("1 keyframes");
    expect(view.bytesLabel).toBe("1.5 KiB");
    expect(view.lastFrameLabel).toBe("#3 delta, 512 B, 99 ms");
    expect(view.healthLabel).toBe("stream negotiated");
    expect(view.hasEncodedFrame).toBe(true);
  });

  it("keeps stopped streams visible without claiming active frame preview", () => {
    const view = buildVideoRendererView(
      {
        ...baseStream,
        state: "stopped",
        encoding: {
          ...baseStream.encoding,
          state: "drained",
        },
        health: {
          healthy: true,
          message: "stream stopped by client",
        },
      },
      null
    );

    expect(view.state).toBe("stopped");
    expect(view.pipelineLabel).toBe("Drained");
    expect(view.emptyHeading).toBe("Stream stopped");
    expect(view.emptyDetail).toBe("The last stream metadata is retained after stop.");
    expect(view.lastFrameLabel).toBe("#3 delta, 512 B, 99 ms");
    expect(view.hasEncodedFrame).toBe(false);
  });

  it("uses failure reason for failed streams", () => {
    const view = buildVideoRendererView(
      {
        ...baseStream,
        state: "failed",
        failureReason: "capture backend failed",
      },
      null
    );

    expect(view.stateLabel).toBe("Failed");
    expect(view.healthLabel).toBe("capture backend failed");
  });
});
