type LogLevel = "debug" | "info" | "warn" | "error";

type LogEntry = {
  ts: number;
  level: LogLevel;
  scope: string;
  message: string;
  data?: string;
};

const STORAGE_KEY = "tsuki:logs";
const MAX_LOGS = 200;

function maskSensitiveText(value: string): string {
  return value
    .replace(/(authorization\s*[:=]\s*)([^,\s]+)/gi, "$1***")
    .replace(/(token\s*[:=]\s*)([^,\s]+)/gi, "$1***");
}

function safeStringify(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  try {
    return JSON.stringify(value, (key, val) => {
      const lowered = key.toLowerCase();
      if (lowered.includes("token") || lowered.includes("authorization")) {
        return "***";
      }
      return val;
    });
  } catch {
    return "[unserializable]";
  }
}

function readLogs(): LogEntry[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed as LogEntry[];
  } catch {
    return [];
  }
}

function writeLogs(entries: LogEntry[]): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(entries));
}

export function log(level: LogLevel, scope: string, message: string, data?: unknown): void {
  const entry: LogEntry = {
    ts: Date.now(),
    level,
    scope,
    message: maskSensitiveText(message),
  };

  const serialized = safeStringify(data);
  if (serialized !== undefined) {
    entry.data = maskSensitiveText(serialized);
  }

  const entries = readLogs();
  entries.push(entry);
  if (entries.length > MAX_LOGS) {
    entries.splice(0, entries.length - MAX_LOGS);
  }
  writeLogs(entries);

  if (level === "error") {
    console.error(`[${scope}] ${entry.message}`, data ?? "");
  } else if (level === "warn") {
    console.warn(`[${scope}] ${entry.message}`, data ?? "");
  } else if (level === "info") {
    console.info(`[${scope}] ${entry.message}`, data ?? "");
  } else {
    console.debug(`[${scope}] ${entry.message}`, data ?? "");
  }
}

export function getLogs(): LogEntry[] {
  return readLogs();
}

export function clearLogs(): void {
  localStorage.removeItem(STORAGE_KEY);
}
