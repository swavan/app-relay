import { invoke } from "@tauri-apps/api/core";

export type ApplicationPermission = {
  applicationId: string;
  label: string;
};

export interface ApplicationPermissionService {
  list(): Promise<ApplicationPermission[]>;
  save(permission: ApplicationPermission): Promise<void>;
  remove(applicationId: string): Promise<void>;
}

export class UIApplicationPermissionService implements ApplicationPermissionService {
  async list(): Promise<ApplicationPermission[]> {
    return invoke<ApplicationPermission[]>("list_application_permissions");
  }

  async save(permission: ApplicationPermission): Promise<void> {
    return invoke<void>("save_application_permission", { permission });
  }

  async remove(applicationId: string): Promise<void> {
    return invoke<void>("remove_application_permission", { applicationId });
  }
}
