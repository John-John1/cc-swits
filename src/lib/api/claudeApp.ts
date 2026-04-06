import { invoke } from "@tauri-apps/api/core";

export interface ClaudeAppBridgeStatus {
  running: boolean;
  providerId?: string | null;
  providerName?: string | null;
  proxyBaseUrl?: string | null;
  proxyMessagesUrl?: string | null;
  launchCommand?: string | null;
  pid?: number | null;
  startedAt?: string | null;
  message?: string | null;
  lastError?: string | null;
}

export const claudeAppApi = {
  async getStatus(): Promise<ClaudeAppBridgeStatus> {
    return await invoke("get_claude_app_bridge_status");
  },

  async activateProvider(providerId: string): Promise<ClaudeAppBridgeStatus> {
    return await invoke("activate_claude_app_provider", { providerId });
  },

  async stopBridge(): Promise<void> {
    await invoke("stop_claude_app_bridge");
  },
};
