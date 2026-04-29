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
  state: "starting" | "streaming" | "stopped" | "failed";
  failureReason?: string;
};
