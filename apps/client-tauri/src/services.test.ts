import { describe, expect, it } from "vitest";
import type { RemoteService } from "./services";

class FakeRemoteService implements RemoteService {
  async health() {
    return {
      service: "swavan-server",
      healthy: true,
      version: "test"
    };
  }

  async capabilities() {
    return [
      {
        platform: "linux",
        feature: "appDiscovery",
        supported: true
      }
    ];
  }

  async applications() {
    return [
      {
        id: "terminal",
        name: "Terminal"
      }
    ];
  }

  async createSession(applicationId: string) {
    return {
      id: "session-1",
      applicationId,
      selectedWindow: {
        id: "window-session-1",
        title: applicationId
      },
      viewport: {
        width: 1280,
        height: 720
      },
      state: "ready" as const
    };
  }

  async resizeSession(sessionId: string) {
    return {
      id: sessionId,
      applicationId: "terminal",
      selectedWindow: {
        id: "window-session-1",
        title: "terminal"
      },
      viewport: {
        width: 1440,
        height: 900
      },
      state: "ready" as const
    };
  }

  async closeSession(sessionId: string) {
    return {
      id: sessionId,
      applicationId: "terminal",
      selectedWindow: {
        id: "window-session-1",
        title: "terminal"
      },
      viewport: {
        width: 1280,
        height: 720
      },
      state: "closed" as const
    };
  }
}

describe("RemoteService contract", () => {
  it("returns server health", async () => {
    const service = new FakeRemoteService();

    await expect(service.health()).resolves.toEqual({
      service: "swavan-server",
      healthy: true,
      version: "test"
    });
  });

  it("returns capabilities and applications", async () => {
    const service = new FakeRemoteService();

    await expect(service.capabilities()).resolves.toHaveLength(1);
    await expect(service.applications()).resolves.toEqual([{ id: "terminal", name: "Terminal" }]);
  });

  it("creates, resizes, and closes sessions", async () => {
    const service = new FakeRemoteService();

    await expect(service.createSession("terminal", { width: 1280, height: 720 })).resolves.toMatchObject({
      id: "session-1",
      applicationId: "terminal",
      state: "ready"
    });
    await expect(service.resizeSession("session-1", { width: 1440, height: 900 })).resolves.toMatchObject({
      id: "session-1",
      viewport: { width: 1440, height: 900 }
    });
    await expect(service.closeSession("session-1")).resolves.toMatchObject({
      id: "session-1",
      state: "closed"
    });
  });
});
