<script lang="ts">

  import { fetch } from '@tauri-apps/plugin-http';
  import { onMount } from 'svelte';
  import { clearLogs, getLogs } from '../lib/logger';

  let config: { endpoint: string, token: string, user: string } = $state(JSON.parse(localStorage.getItem("config") ?? "{}"));
  let serverMetadata = $state("");
  let logs = $state<Array<{ ts: number; level: string; scope: string; message: string; data?: string }>>([]);

  function secure(): "s" | "" {
    return ((config.endpoint && config.endpoint.match(/^localhost|^10\.0\.2\.2/)) ? "" : "s");
  }

  function formatLog(entry: { ts: number; level: string; scope: string; message: string; data?: string }): string {
    const time = new Date(entry.ts).toISOString();
    const data = entry.data ? ` ${entry.data}` : "";
    return `[${time}] ${entry.level.toUpperCase()} ${entry.scope} ${entry.message}${data}`;
  }

  function loadLogs() {
    logs = getLogs().toReversed();
  }

  function handleClearLogs() {
    clearLogs();
    loadLogs();
  }

  onMount(() => {
    fetch(`http${secure()}://${config.endpoint}/metadata`, {
      headers: {
        "Authorization": `${config.user}:${config.token}`,
      }
    })
      .then(response => response.json())
      .then(json => {
        serverMetadata = JSON.stringify(json, null, "  ");
      })
      .catch(error => {
        serverMetadata = `fetch error: ${error}`;
      });

    loadLogs();
    const intervalId = setInterval(loadLogs, 1000);
    return () => {
      clearInterval(intervalId);
    };
  });

</script>

<div class="status-box">
  <div class="field">
    <label for="serverMetadata">Server metadata</label>
    <pre id="serverMetadata" class="metadata">{serverMetadata}</pre>
  </div>
  <div class="field">
    <div class="log-header">
      <label for="localLogs">Local logs</label>
      <button class="clear-button" onclick={handleClearLogs}>Clear</button>
    </div>
    <div id="localLogs" class="logs">
      {#each logs as entry}
        <div class="log-entry">{formatLog(entry)}</div>
      {/each}
    </div>
  </div>
</div>

<style>

.status-box {
  font-size: 0.8rem;
  font-weight: 400;
  line-height: 2rem;
}

.field {
  display: flex;
  flex-direction: column;
}

.metadata {
  background-color: #eee;
  outline: none;
  border: none;
  font-size: 0.65rem;
  line-height: 0.8rem;
  padding: 0.2rem;
  width: 98%;
  height: 8.5rem;
  overflow: scroll;
  margin: 0;
}

.log-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.clear-button {
  background-color: #ddd;
  border: none;
  border-radius: 4px;
  font-size: 0.65rem;
  padding: 0.1rem 0.4rem;
}

.logs {
  background-color: #eee;
  outline: none;
  border: none;
  font-size: 0.6rem;
  line-height: 0.85rem;
  padding: 0.2rem;
  width: 98%;
  height: 10rem;
  overflow: scroll;
  margin: 0;
}

.log-entry {
  padding: 0.1rem 0;
  white-space: pre-wrap;
}

</style>
