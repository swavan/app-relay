import { beforeEach, describe, expect, it, vi } from "vitest";
import { TauriRemoteService } from "./services";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke
}));

describe("TauriRemoteService", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
  });

  it("sends the configured profile token through the test server control-plane path", async () => {
    tauri.invoke
      .mockResolvedValueOnce({
        service: "apprelay-server",
        healthy: true,
        version: "mobile-contract-test"
      })
      .mockResolvedValueOnce([
        {
          platform: "linux",
          feature: "appDiscovery",
          supported: true
        }
      ])
      .mockResolvedValueOnce([
        {
          id: "terminal",
          name: "Terminal"
        }
      ])
      .mockResolvedValueOnce([]);

    const service = new TauriRemoteService("mobile-test-token");

    await expect(service.health()).resolves.toMatchObject({
      service: "apprelay-server",
      healthy: true
    });
    await expect(service.capabilities()).resolves.toHaveLength(1);
    await expect(service.applications()).resolves.toEqual([{ id: "terminal", name: "Terminal" }]);
    await expect(service.activeSessions()).resolves.toEqual([]);

    expect(tauri.invoke.mock.calls).toEqual([
      ["server_health", { authToken: "mobile-test-token" }],
      ["server_capabilities", { authToken: "mobile-test-token" }],
      ["server_applications", { authToken: "mobile-test-token" }],
      ["active_application_sessions", { authToken: "mobile-test-token" }]
    ]);
  });
});
