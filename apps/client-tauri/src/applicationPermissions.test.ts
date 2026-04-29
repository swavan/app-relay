import { describe, expect, it } from "vitest";
import type { ApplicationPermission, ApplicationPermissionService } from "./applicationPermissions";

class FakeApplicationPermissionService implements ApplicationPermissionService {
  private permissions: ApplicationPermission[] = [];

  async list(): Promise<ApplicationPermission[]> {
    return [...this.permissions].sort((left, right) =>
      left.label.localeCompare(right.label) || left.applicationId.localeCompare(right.applicationId)
    );
  }

  async save(permission: ApplicationPermission): Promise<void> {
    this.permissions = this.permissions.filter(
      (existing) => existing.applicationId !== permission.applicationId
    );
    this.permissions.push(permission);
  }

  async remove(applicationId: string): Promise<void> {
    this.permissions = this.permissions.filter(
      (permission) => permission.applicationId !== applicationId
    );
  }
}

describe("ApplicationPermissionService contract", () => {
  it("lists, replaces, and removes application permissions", async () => {
    const service = new FakeApplicationPermissionService();

    await service.save({ applicationId: "zed", label: "Zed" });
    await service.save({ applicationId: "terminal", label: "Terminal" });
    await service.save({ applicationId: "terminal", label: "Terminal Updated" });

    await expect(service.list()).resolves.toEqual([
      { applicationId: "terminal", label: "Terminal Updated" },
      { applicationId: "zed", label: "Zed" }
    ]);

    await service.remove("terminal");

    await expect(service.list()).resolves.toEqual([{ applicationId: "zed", label: "Zed" }]);
  });
});
