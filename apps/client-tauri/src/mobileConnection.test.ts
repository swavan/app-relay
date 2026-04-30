import { describe, expect, it } from "vitest";
import type { ConnectionProfile } from "./connectionProfiles";
import { connectMobileClientToTestServer, type MobileClientTarget } from "./mobileConnection";
import type { RemoteService, ViewportSize, InputEvent } from "./services";
import type { AudioStreamStartOptions, AudioStreamUpdate } from "./audioStreams";
import type { WebRtcIceCandidate, WebRtcSessionDescription } from "./videoStreams";

const mobileTestProfile: ConnectionProfile = {
  id: "mobile-test-server",
  label: "Mobile Test Server",
  sshUser: "apprelay",
  sshHost: "test-server.local",
  localPort: 49152,
  remotePort: 9843,
  authToken: "mobile-test-token"
};

class MobileTestRemoteService implements RemoteService {
  readonly calls: string[] = [];

  constructor(private readonly expectedToken: string) {}

  async health() {
    this.calls.push(`health:${this.expectedToken}`);
    return {
      service: "apprelay-server",
      healthy: true,
      version: "mobile-contract-test"
    };
  }

  async capabilities() {
    this.calls.push(`capabilities:${this.expectedToken}`);
    return [
      {
        platform: "linux",
        feature: "appDiscovery",
        supported: true
      }
    ];
  }

  async applications() {
    this.calls.push(`applications:${this.expectedToken}`);
    return [
      {
        id: "terminal",
        name: "Terminal"
      }
    ];
  }

  async activeSessions() {
    this.calls.push(`activeSessions:${this.expectedToken}`);
    return [
      {
        id: "session-1",
        applicationId: "terminal",
        selectedWindow: {
          id: "window-session-1",
          applicationId: "terminal",
          title: "Terminal",
          selectionMethod: "synthetic" as const
        },
        viewport: {
          width: 1280,
          height: 720
        },
        state: "ready" as const
      }
    ];
  }

  async createSession(_applicationId: string, _viewport: ViewportSize): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async resizeSession(_sessionId: string, _viewport: ViewportSize): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async closeSession(_sessionId: string): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async forwardInput(
    _sessionId: string,
    _clientViewport: ViewportSize,
    _event: InputEvent
  ): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async startVideoStream(_sessionId: string): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async stopVideoStream(_streamId: string): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async startAudioStream(
    _sessionId: string,
    _options: AudioStreamStartOptions
  ): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async stopAudioStream(_streamId: string): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async updateAudioStream(_streamId: string, _update: AudioStreamUpdate): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async audioStreamStatus(_streamId: string): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async reconnectVideoStream(_streamId: string): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async negotiateVideoStream(
    _streamId: string,
    _clientAnswer: WebRtcSessionDescription,
    _clientIceCandidates: WebRtcIceCandidate[]
  ): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }

  async videoStreamStatus(_streamId: string): Promise<never> {
    throw new Error("not used by mobile connection contract");
  }
}

describe("connectMobileClientToTestServer", () => {
  it.each<MobileClientTarget>(["android", "ios"])(
    "uses the configured profile token for the %s control-plane path",
    async (target) => {
      const service = new MobileTestRemoteService(mobileTestProfile.authToken);

      await expect(
        connectMobileClientToTestServer(target, mobileTestProfile, (profile) => {
          expect(profile).toEqual(mobileTestProfile);
          return service;
        })
      ).resolves.toEqual({
        target,
        profileId: "mobile-test-server",
        profileLabel: "Mobile Test Server",
        health: {
          service: "apprelay-server",
          healthy: true,
          version: "mobile-contract-test"
        },
        capabilities: [
          {
            platform: "linux",
            feature: "appDiscovery",
            supported: true
          }
        ],
        applications: [
          {
            id: "terminal",
            name: "Terminal"
          }
        ],
        activeSessions: [
          {
            id: "session-1",
            applicationId: "terminal",
            selectedWindow: {
              id: "window-session-1",
              applicationId: "terminal",
              title: "Terminal",
              selectionMethod: "synthetic"
            },
            viewport: {
              width: 1280,
              height: 720
            },
            state: "ready"
          }
        ]
      });

      expect(service.calls).toEqual([
        "health:mobile-test-token",
        "capabilities:mobile-test-token",
        "applications:mobile-test-token",
        "activeSessions:mobile-test-token"
      ]);
    }
  );

  it("fails before contacting the server when the profile has no token", async () => {
    await expect(
      connectMobileClientToTestServer("android", {
        ...mobileTestProfile,
        authToken: " "
      })
    ).rejects.toThrow("mobile test server profile must include a control-plane auth token");
  });
});
