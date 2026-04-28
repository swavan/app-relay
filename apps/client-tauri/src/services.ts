export type HealthStatus = {
  service: string;
  healthy: boolean;
  version: string;
};

export type AppSummary = {
  id: string;
  name: string;
  iconUrl?: string;
};

export interface RemoteService {
  health(): Promise<HealthStatus>;
  applications(): Promise<AppSummary[]>;
}

export class UnsupportedRemoteService implements RemoteService {
  async health(): Promise<HealthStatus> {
    return {
      service: "swavan-server",
      healthy: false,
      version: "unconnected"
    };
  }

  async applications(): Promise<AppSummary[]> {
    return [];
  }
}

