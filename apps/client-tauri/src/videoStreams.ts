import type { ViewportSize } from "./services";

export type WebRtcSessionDescription = {
  sdpType: "offer" | "answer";
  sdp: string;
};

export type WebRtcIceCandidate = {
  candidate: string;
  sdpMid?: string;
  sdpMLineIndex?: number;
};

export type VideoEncodingPipeline = {
  contract: {
    codec: "h264";
    pixelFormat: "rgba";
    hardwareAcceleration: "none";
    target: {
      resolution: ViewportSize;
      maxFps: number;
      targetBitrateKbps: number;
      keyframeIntervalFrames: number;
    };
    adaptation: {
      requestedViewport: ViewportSize;
      currentTarget: ViewportSize;
      limits: {
        maxWidth: number;
        maxHeight: number;
        maxPixels: number;
      };
      reason: "matchesViewport" | "cappedToLimits";
    };
  };
  state: "configured" | "encoding" | "drained";
  output: {
    framesSubmitted: number;
    framesEncoded: number;
    keyframesEncoded: number;
    bytesProduced: number;
    lastFrame: {
      sequence: number;
      timestampMs: number;
      byteLength: number;
      keyframe: boolean;
    } | null;
  };
};

export type VideoCaptureRuntimeStatus = {
  state:
    | "unavailable"
    | "starting"
    | "delivering"
    | "stopped"
    | "failed"
    | "permissionDenied";
  framesDelivered: number;
  lastFrame: {
    sequence: number;
    timestampMs: number;
    size: ViewportSize;
  } | null;
  message?: string;
};

export type VideoStreamSession = {
  id: string;
  sessionId: string;
  selectedWindowId: string;
  viewport: ViewportSize;
  captureSource: {
    scope: "selectedWindow";
    selectedWindowId: string;
    applicationId: string;
    title: string;
  };
  captureRuntime?: VideoCaptureRuntimeStatus;
  encoding: VideoEncodingPipeline;
  signaling: {
    kind: "webRtcOffer";
    negotiationState: "awaitingAnswer" | "negotiated";
    offer?: WebRtcSessionDescription;
    answer?: WebRtcSessionDescription;
    iceCandidates: WebRtcIceCandidate[];
  };
  stats: {
    framesEncoded: number;
    bitrateKbps: number;
    latencyMs: number;
    reconnectAttempts: number;
  };
  health: {
    healthy: boolean;
    message?: string;
  };
  failure?: {
    kind: "appClosed" | "captureFailed";
    message: string;
    recovery: {
      action: "reconnectStream" | "restartApplicationSession" | "none";
      message: string;
      retryable: boolean;
    };
  };
  state: "starting" | "streaming" | "stopped" | "failed";
  failureReason?: string;
};
