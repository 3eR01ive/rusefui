export interface ConnectionStatus {
  connected: boolean;
  port_name?: string | null;
  baud_rate?: number | null;
  signature?: string | null;
  handshake_command?: string | null;
  last_error?: string | null;
}

export interface ConnectParams {
  port: string;
  baud_rate?: number;
  timeout_ms?: number;
}
