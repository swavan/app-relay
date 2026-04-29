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
