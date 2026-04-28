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
