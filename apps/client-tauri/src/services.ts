import { invoke } from "@tauri-apps/api/core";
import type {
  AudioStreamSession,
  AudioStreamStartOptions,
  AudioStreamUpdate
} from "./audioStreams";
import type {
  VideoStreamSession,
  WebRtcIceCandidate,
  WebRtcSessionDescription
} from "./videoStreams";

export type HealthStatus = {
  service: string;
  healthy: boolean;
  version: string;
};

export type Capability = {
  platform: string;
  feature: string;
  supported: boolean;
  reason?: string;
};

export type AppSummary = {
  id: string;
  name: string;
  icon?: {
    mimeType: string;
    dataUrl?: string;
    source?: string;
  };
  launch?: {
    kind: "desktopCommand" | "macosBundle";
    value: string;
  };
};

export type ViewportSize = {
  width: number;
  height: number;
};

export type ApplicationSession = {
  id: string;
  applicationId: string;
  selectedWindow: {
    id: string;
    applicationId: string;
    title: string;
    selectionMethod: "launchIntent" | "existingWindow" | "nativeWindow" | "synthetic";
  };
  launchIntent?: {
    sessionId: string;
    applicationId: string;
    launch?: {
      kind: "desktopCommand" | "macosBundle";
      value: string;
    };
    status: "recorded" | "attached" | "unsupported";
  };
  viewport: ViewportSize;
  resizeIntent?: {
    sessionId: string;
    selectedWindowId: string;
    viewport: ViewportSize;
    status: "recorded" | "applied" | "unsupported";
  };
  state: "starting" | "ready" | "closed";
};

export type ClientPoint = {
  x: number;
  y: number;
};

export type PointerButton = "primary" | "secondary" | "middle";
export type ButtonAction = "press" | "release";
export type KeyAction = "press" | "release";

export type KeyModifiers = {
  shift: boolean;
  control: boolean;
  alt: boolean;
  meta: boolean;
};

export type InputEvent =
  | { kind: "focus" }
  | { kind: "blur" }
  | { kind: "pointerMove"; position: ClientPoint }
  | {
      kind: "pointerButton";
      position: ClientPoint;
      button: PointerButton;
      action: ButtonAction;
    }
  | { kind: "pointerScroll"; position: ClientPoint; deltaX: number; deltaY: number }
  | { kind: "pointerDrag"; from: ClientPoint; to: ClientPoint; button: PointerButton }
  | { kind: "keyboardText"; text: string }
  | { kind: "keyboardKey"; key: string; action: KeyAction; modifiers: KeyModifiers };

export type ServerPoint = {
  x: number;
  y: number;
};

export type MappedInputEvent =
  | { kind: "focus" }
  | { kind: "blur" }
  | { kind: "pointerMove"; position: ServerPoint }
  | {
      kind: "pointerButton";
      position: ServerPoint;
      button: PointerButton;
      action: ButtonAction;
    }
  | { kind: "pointerScroll"; position: ServerPoint; deltaX: number; deltaY: number }
  | { kind: "pointerDrag"; from: ServerPoint; to: ServerPoint; button: PointerButton }
  | { kind: "keyboardText"; text: string }
  | { kind: "keyboardKey"; key: string; action: KeyAction; modifiers: KeyModifiers };

export type InputDelivery = {
  sessionId: string;
  selectedWindowId: string;
  mappedEvent: MappedInputEvent;
  status: "focused" | "blurred" | "delivered" | "ignoredBlurred";
};

export interface RemoteService {
  health(): Promise<HealthStatus>;
  capabilities(): Promise<Capability[]>;
  applications(): Promise<AppSummary[]>;
  activeSessions(): Promise<ApplicationSession[]>;
  createSession(applicationId: string, viewport: ViewportSize): Promise<ApplicationSession>;
  resizeSession(sessionId: string, viewport: ViewportSize): Promise<ApplicationSession>;
  closeSession(sessionId: string): Promise<ApplicationSession>;
  forwardInput(
    sessionId: string,
    clientViewport: ViewportSize,
    event: InputEvent
  ): Promise<InputDelivery>;
  activeVideoStreams(): Promise<VideoStreamSession[]>;
  startVideoStream(sessionId: string): Promise<VideoStreamSession>;
  stopVideoStream(streamId: string): Promise<VideoStreamSession>;
  activeAudioStreams(): Promise<AudioStreamSession[]>;
  startAudioStream(
    sessionId: string,
    options: AudioStreamStartOptions
  ): Promise<AudioStreamSession>;
  stopAudioStream(streamId: string): Promise<AudioStreamSession>;
  updateAudioStream(streamId: string, update: AudioStreamUpdate): Promise<AudioStreamSession>;
  audioStreamStatus(streamId: string): Promise<AudioStreamSession>;
  reconnectVideoStream(streamId: string): Promise<VideoStreamSession>;
  negotiateVideoStream(
    streamId: string,
    clientAnswer: WebRtcSessionDescription,
    clientIceCandidates: WebRtcIceCandidate[]
  ): Promise<VideoStreamSession>;
  videoStreamStatus(streamId: string): Promise<VideoStreamSession>;
}

function streamHydrationRank(stream: VideoStreamSession) {
  switch (stream.state) {
    case "streaming":
      return 3;
    case "starting":
      return 2;
    case "failed":
      return 1;
    default:
      return 0;
  }
}

export function selectHydratedVideoStream(
  activeStreams: VideoStreamSession[],
  sessionId: string
): VideoStreamSession | null {
  return (
    activeStreams
      .filter((stream) => stream.sessionId === sessionId && stream.state !== "stopped")
      .sort((left, right) => streamHydrationRank(right) - streamHydrationRank(left))[0] ?? null
  );
}

export async function hydrateActiveVideoStream(
  remote: Pick<RemoteService, "activeVideoStreams">,
  sessionId: string,
  onError: (message: string) => void
): Promise<VideoStreamSession | null> {
  try {
    return selectHydratedVideoStream(await remote.activeVideoStreams(), sessionId);
  } catch (error) {
    onError(error instanceof Error ? error.message : String(error));
    return null;
  }
}

export function selectHydratedAudioStream(
  activeStreams: AudioStreamSession[],
  sessionId: string
): AudioStreamSession | null {
  return (
    activeStreams
      .map((stream, index) => ({ stream, index }))
      .filter(({ stream }) => stream.sessionId === sessionId && stream.state !== "stopped")
      .sort((left, right) => {
        if (left.stream.state !== right.stream.state) {
          return left.stream.state === "streaming" ? -1 : 1;
        }

        return right.index - left.index;
      })[0]?.stream ?? null
  );
}

export async function hydrateActiveAudioStream(
  remote: Pick<RemoteService, "activeAudioStreams">,
  sessionId: string,
  onError: (message: string) => void
): Promise<AudioStreamSession | null> {
  try {
    return selectHydratedAudioStream(await remote.activeAudioStreams(), sessionId);
  } catch (error) {
    onError(error instanceof Error ? error.message : String(error));
    return null;
  }
}

export class TauriRemoteService implements RemoteService {
  private readonly authToken: string;
  private readonly clientId: string;

  constructor(authToken?: string, clientId?: string) {
    this.authToken = authToken ?? "local-dev-token";
    this.clientId = clientId ?? "local-dev-client";
  }

  async health(): Promise<HealthStatus> {
    return invoke<HealthStatus>("server_health", { authToken: this.authToken });
  }

  async capabilities(): Promise<Capability[]> {
    return invoke<Capability[]>("server_capabilities", { authToken: this.authToken });
  }

  async applications(): Promise<AppSummary[]> {
    return invoke<AppSummary[]>("server_applications", { authToken: this.authToken });
  }

  async activeSessions(): Promise<ApplicationSession[]> {
    return invoke<ApplicationSession[]>("active_application_sessions", {
      authToken: this.authToken,
      clientId: this.clientId
    });
  }

  async createSession(
    applicationId: string,
    viewport: ViewportSize
  ): Promise<ApplicationSession> {
    return invoke<ApplicationSession>("create_application_session", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { applicationId, viewport }
    });
  }

  async resizeSession(sessionId: string, viewport: ViewportSize): Promise<ApplicationSession> {
    return invoke<ApplicationSession>("resize_application_session", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { sessionId, viewport }
    });
  }

  async closeSession(sessionId: string): Promise<ApplicationSession> {
    return invoke<ApplicationSession>("close_application_session", {
      authToken: this.authToken,
      clientId: this.clientId,
      sessionId
    });
  }

  async forwardInput(
    sessionId: string,
    clientViewport: ViewportSize,
    event: InputEvent
  ): Promise<InputDelivery> {
    return invoke<InputDelivery>("forward_input", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { sessionId, clientViewport, event }
    });
  }

  async activeVideoStreams(): Promise<VideoStreamSession[]> {
    return invoke<VideoStreamSession[]>("active_video_streams", {
      authToken: this.authToken,
      clientId: this.clientId
    });
  }

  async startVideoStream(sessionId: string): Promise<VideoStreamSession> {
    return invoke<VideoStreamSession>("start_video_stream", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { sessionId }
    });
  }

  async stopVideoStream(streamId: string): Promise<VideoStreamSession> {
    return invoke<VideoStreamSession>("stop_video_stream", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { streamId }
    });
  }

  async activeAudioStreams(): Promise<AudioStreamSession[]> {
    return invoke<AudioStreamSession[]>("active_audio_streams", {
      authToken: this.authToken,
      clientId: this.clientId
    });
  }

  async startAudioStream(
    sessionId: string,
    options: AudioStreamStartOptions
  ): Promise<AudioStreamSession> {
    return invoke<AudioStreamSession>("start_audio_stream", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { sessionId, ...options }
    });
  }

  async stopAudioStream(streamId: string): Promise<AudioStreamSession> {
    return invoke<AudioStreamSession>("stop_audio_stream", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { streamId }
    });
  }

  async updateAudioStream(
    streamId: string,
    update: AudioStreamUpdate
  ): Promise<AudioStreamSession> {
    return invoke<AudioStreamSession>("update_audio_stream", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { streamId, ...update }
    });
  }

  async audioStreamStatus(streamId: string): Promise<AudioStreamSession> {
    return invoke<AudioStreamSession>("audio_stream_status", {
      authToken: this.authToken,
      clientId: this.clientId,
      streamId
    });
  }

  async reconnectVideoStream(streamId: string): Promise<VideoStreamSession> {
    return invoke<VideoStreamSession>("reconnect_video_stream", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { streamId }
    });
  }

  async negotiateVideoStream(
    streamId: string,
    clientAnswer: WebRtcSessionDescription,
    clientIceCandidates: WebRtcIceCandidate[]
  ): Promise<VideoStreamSession> {
    return invoke<VideoStreamSession>("negotiate_video_stream", {
      authToken: this.authToken,
      clientId: this.clientId,
      request: { streamId, clientAnswer, clientIceCandidates }
    });
  }

  async videoStreamStatus(streamId: string): Promise<VideoStreamSession> {
    return invoke<VideoStreamSession>("video_stream_status", {
      authToken: this.authToken,
      clientId: this.clientId,
      streamId
    });
  }
}
