export type ClientConfig = {
  showInternalMessages: boolean;
};

export const CLIENT_CONFIG_STORAGE_KEY = "clientConfig";
export const CLIENT_CONFIG_UPDATED_EVENT = "tsuki:client-config-updated";

const DEFAULT_CLIENT_CONFIG: ClientConfig = {
  showInternalMessages: true,
};

function normalizeClientConfig(value: unknown): ClientConfig {
  if (typeof value !== "object" || value === null) {
    return { ...DEFAULT_CLIENT_CONFIG };
  }
  const candidate = value as Partial<ClientConfig>;
  return {
    showInternalMessages: typeof candidate.showInternalMessages === "boolean"
      ? candidate.showInternalMessages
      : DEFAULT_CLIENT_CONFIG.showInternalMessages,
  };
}

export function loadClientConfig(): ClientConfig {
  try {
    const raw = localStorage.getItem(CLIENT_CONFIG_STORAGE_KEY);
    if (!raw) {
      return { ...DEFAULT_CLIENT_CONFIG };
    }
    return normalizeClientConfig(JSON.parse(raw));
  } catch {
    return { ...DEFAULT_CLIENT_CONFIG };
  }
}

export function saveClientConfig(config: ClientConfig): void {
  const normalized = normalizeClientConfig(config);
  localStorage.setItem(CLIENT_CONFIG_STORAGE_KEY, JSON.stringify(normalized));
  window.dispatchEvent(new CustomEvent<ClientConfig>(CLIENT_CONFIG_UPDATED_EVENT, {
    detail: normalized,
  }));
}
