export type MicrophoneMode = "disabled" | "enabled";

export type AudioStreamSession = {
  id: string;
  sessionId: string;
  selectedWindowId: string;
  source: {
    scope: "selectedApplication" | "system";
    selectedWindowId: string;
    applicationId: string;
    title: string;
  };
  devices: {
    outputDeviceId?: string;
    inputDeviceId?: string;
  };
  microphone: MicrophoneMode;
  mute: {
    systemAudioMuted: boolean;
    microphoneMuted: boolean;
  };
  capabilities: {
    systemAudio: AudioCapability;
    microphoneCapture: AudioCapability;
    microphoneInjection: AudioCapability;
    echoCancellation: AudioCapability;
    deviceSelection: AudioCapability;
  };
  stats: {
    packetsSent: number;
    packetsReceived: number;
    latencyMs: number;
  };
  health: {
    healthy: boolean;
    message?: string;
  };
  state: "starting" | "streaming" | "stopped";
};

export type AudioCapability = {
  supported: boolean;
  reason?: string;
};

export type AudioStreamStartOptions = {
  microphone: MicrophoneMode;
  systemAudioMuted: boolean;
  microphoneMuted: boolean;
  outputDeviceId?: string;
  inputDeviceId?: string;
};

export type AudioStreamUpdate = Omit<AudioStreamStartOptions, "microphone">;
