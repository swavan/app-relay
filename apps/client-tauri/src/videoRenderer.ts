import type { ViewportSize } from "./services";
import type { VideoStreamSession } from "./videoStreams";

export type VideoRendererView = {
  state: VideoRendererState;
  stateLabel: string;
  windowTitle: string;
  streamIdLabel: string;
  selectedWindowLabel: string;
  requestedResolutionLabel: string;
  targetResolutionLabel: string;
  viewportResolutionLabel: string;
  adaptationLabel: string;
  codecLabel: string;
  pipelineLabel: string;
  bitrateLabel: string;
  frameRateLabel: string;
  framesLabel: string;
  keyframesLabel: string;
  bytesLabel: string;
  lastFrameLabel: string;
  healthLabel: string;
  reconnectsLabel: string;
  latencyLabel: string;
  recoveryLabel: string;
  emptyHeading: string;
  emptyDetail: string;
  hasEncodedFrame: boolean;
};

export type VideoRendererState =
  | "empty"
  | "starting"
  | "streaming"
  | "stopped"
  | "failed";

export function buildVideoRendererView(
  stream: VideoStreamSession | null,
  requestedViewport: ViewportSize | null
): VideoRendererView {
  if (!stream) {
    const requestedResolutionLabel = requestedViewport
      ? formatResolution(requestedViewport)
      : "No requested viewport";

    return {
      state: "empty",
      stateLabel: "No stream",
      windowTitle: "No selected stream",
      streamIdLabel: "Stream not started",
      selectedWindowLabel: "Selected window unavailable",
      requestedResolutionLabel,
      targetResolutionLabel: "No encoding target",
      viewportResolutionLabel: requestedResolutionLabel,
      adaptationLabel: "No adaptation metadata",
      codecLabel: "No codec configured",
      pipelineLabel: "Idle",
      bitrateLabel: "0 kbps",
      frameRateLabel: "0 fps",
      framesLabel: "0 frames",
      keyframesLabel: "0 keyframes",
      bytesLabel: "0 B",
      lastFrameLabel: "No encoded frames",
      healthLabel: "Waiting for stream start",
      reconnectsLabel: "0 reconnects",
      latencyLabel: "0 ms",
      recoveryLabel: "No recovery action",
      emptyHeading: "Stream not started",
      emptyDetail: "Start a stream to show selected-window stream metadata.",
      hasEncodedFrame: false,
    };
  }

  const target = stream.encoding.contract.target;
  const adaptation = stream.encoding.contract.adaptation;
  const lastFrame = stream.encoding.output.lastFrame;
  const healthLabel =
    stream.failure?.message ??
    stream.failureReason ??
    stream.health.message ??
    (stream.health.healthy ? "Stream metadata healthy" : "Stream metadata unhealthy");
  const recoveryLabel = stream.failure
    ? stream.failure.recovery.message
    : "No recovery action";

  return {
    state: stream.state,
    stateLabel: streamStateLabel(stream.state),
    windowTitle: stream.captureSource.title || "Untitled selected window",
    streamIdLabel: stream.id,
    selectedWindowLabel: stream.selectedWindowId,
    requestedResolutionLabel: formatResolution(adaptation.requestedViewport),
    targetResolutionLabel: formatResolution(adaptation.currentTarget),
    viewportResolutionLabel: formatResolution(stream.viewport),
    adaptationLabel: adaptationReasonLabel(adaptation.reason),
    codecLabel: `${target.resolution.width} x ${target.resolution.height} ${stream.encoding.contract.codec.toUpperCase()} / ${stream.encoding.contract.pixelFormat.toUpperCase()}`,
    pipelineLabel: pipelineStateLabel(stream.encoding.state),
    bitrateLabel: `${target.targetBitrateKbps} kbps target`,
    frameRateLabel: `${target.maxFps} fps max`,
    framesLabel: `${stream.encoding.output.framesEncoded} encoded`,
    keyframesLabel: `${stream.encoding.output.keyframesEncoded} keyframes`,
    bytesLabel: formatBytes(stream.encoding.output.bytesProduced),
    lastFrameLabel: lastFrame ? formatLastFrame(lastFrame) : "No encoded frames",
    healthLabel,
    reconnectsLabel: `${stream.stats.reconnectAttempts} reconnects`,
    latencyLabel: `${stream.stats.latencyMs} ms latency`,
    recoveryLabel,
    emptyHeading:
      stream.state === "stopped"
        ? "Stream stopped"
        : stream.state === "failed"
          ? "Stream failed"
          : "No decoded frame",
    emptyDetail: streamEmptyDetail(stream, recoveryLabel),
    hasEncodedFrame: lastFrame !== null && stream.state !== "stopped" && stream.state !== "failed",
  };
}

function streamEmptyDetail(stream: VideoStreamSession, recoveryLabel: string): string {
  if (stream.state === "stopped") {
    return "The last stream metadata is retained after stop.";
  }

  if (stream.state === "failed") {
    return recoveryLabel;
  }

  return "Current pipeline exposes encoded metadata only.";
}

function streamStateLabel(state: VideoRendererState): string {
  switch (state) {
    case "starting":
      return "Starting";
    case "streaming":
      return "Streaming";
    case "stopped":
      return "Stopped";
    case "failed":
      return "Failed";
    case "empty":
      return "No stream";
  }
}

function pipelineStateLabel(state: VideoStreamSession["encoding"]["state"]): string {
  switch (state) {
    case "configured":
      return "Configured";
    case "encoding":
      return "Encoding";
    case "drained":
      return "Drained";
  }
}

function adaptationReasonLabel(
  reason: VideoStreamSession["encoding"]["contract"]["adaptation"]["reason"]
): string {
  switch (reason) {
    case "matchesViewport":
      return "Target matches requested viewport";
    case "cappedToLimits":
      return "Target capped to encoder limits";
  }
}

function formatResolution(viewport: ViewportSize): string {
  return `${viewport.width} x ${viewport.height}`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  const kib = bytes / 1024;
  if (kib < 1024) {
    return `${kib.toFixed(1)} KiB`;
  }

  return `${(kib / 1024).toFixed(1)} MiB`;
}

function formatLastFrame(
  frame: NonNullable<VideoStreamSession["encoding"]["output"]["lastFrame"]>
): string {
  const frameKind = frame.keyframe ? "keyframe" : "delta";
  return `#${frame.sequence} ${frameKind}, ${frame.byteLength} B, ${frame.timestampMs} ms`;
}
