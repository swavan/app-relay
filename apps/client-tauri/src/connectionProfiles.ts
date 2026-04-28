import { invoke } from "@tauri-apps/api/core";

export type ConnectionProfile = {
  id: string;
  label: string;
  sshUser: string;
  sshHost: string;
  localPort: number;
  remotePort: number;
  authToken: string;
};

export interface ConnectionProfileService {
  list(): Promise<ConnectionProfile[]>;
  save(profile: ConnectionProfile): Promise<void>;
  remove(id: string): Promise<void>;
}

export class TauriConnectionProfileService implements ConnectionProfileService {
  async list(): Promise<ConnectionProfile[]> {
    return invoke<ConnectionProfile[]>("list_connection_profiles");
  }

  async save(profile: ConnectionProfile): Promise<void> {
    return invoke<void>("save_connection_profile", { profile });
  }

  async remove(id: string): Promise<void> {
    return invoke<void>("remove_connection_profile", { id });
  }
}
