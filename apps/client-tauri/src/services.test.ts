import { describe, expect, it } from "vitest";
import { UnsupportedRemoteService } from "./services";

describe("UnsupportedRemoteService", () => {
  it("returns an unhealthy status before a server is configured", async () => {
    const service = new UnsupportedRemoteService();

    await expect(service.health()).resolves.toEqual({
      service: "swavan-server",
      healthy: false,
      version: "unconnected"
    });
  });

  it("returns no applications until discovery is implemented", async () => {
    const service = new UnsupportedRemoteService();

    await expect(service.applications()).resolves.toEqual([]);
  });
});

