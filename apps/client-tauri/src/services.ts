import { invoke } from "@tauri-apps/api/core";

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
  iconUrl?: string;
};

export interface RemoteService {
  health(): Promise<HealthStatus>;
  capabilities(): Promise<Capability[]>;
  applications(): Promise<AppSummary[]>;
}

export class TauriRemoteService implements RemoteService {
  private readonly authToken: string;

  constructor(authToken?: string) {
    this.authToken = authToken ?? "local-dev-token";
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
}
