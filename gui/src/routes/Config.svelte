<script lang="ts">

  import { fetch } from '@tauri-apps/plugin-http';
  import { onMount } from 'svelte';
  import { log } from '../lib/logger';

  let config: { endpoint: string, token: string, user: string } = $state(JSON.parse(localStorage.getItem("config") ?? "{}"));
  let runtimeConfig: { enableNotification: boolean, enableSensory: boolean } = $state({
    enableNotification: true,
    enableSensory: true,
  });
  let runtimeError: string = $state("");
  let runtimeSaving: boolean = $state(false);

  $effect(() => {
    localStorage.setItem("config", JSON.stringify(config));
  });

  function secure(): "s" | "" {
    return ((config.endpoint && config.endpoint.match(/^localhost|^10\.0\.2\.2/)) ? "" : "s");
  }

  function hasAuthConfig(): boolean {
    return Boolean(config.endpoint && config.token && config.user);
  }

  function loadRuntimeConfig() {
    runtimeError = "";
    if (!hasAuthConfig()) {
      runtimeError = "Set endpoint, token, and user first.";
      return;
    }
    log("debug", "http", "Runtime config request.");
    fetch(`http${secure()}://${config.endpoint}/config`, {
      headers: {
        "Authorization": `${config.user}:${config.token}`,
      }
    })
      .then(response => response.json())
      .then(data => {
        log("debug", "http", "Runtime config payload received.", data);
        runtimeConfig = {
          enableNotification: Boolean(data.enableNotification),
          enableSensory: Boolean(data.enableSensory),
        };
      })
      .catch(error => {
        runtimeError = error.toString();
        log("error", "http", "Failed to load runtime config.", error);
      });
  }

  function saveRuntimeConfig() {
    runtimeError = "";
    if (!hasAuthConfig()) {
      runtimeError = "Set endpoint, token, and user first.";
      return;
    }
    runtimeSaving = true;
    log("debug", "http", "Runtime config payload sent.", runtimeConfig);
    fetch(`http${secure()}://${config.endpoint}/config`, {
      method: "PUT",
      headers: {
        "Authorization": `${config.user}:${config.token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(runtimeConfig),
    })
      .then(response => {
        if (response.status < 200 || response.status >= 300) {
          throw new Error(`Failed to update config: ${response.status} ${response.statusText}`);
        }
        return response.json();
      })
      .then(data => {
        log("debug", "http", "Runtime config payload received.", data);
        runtimeConfig = {
          enableNotification: Boolean(data.enableNotification),
          enableSensory: Boolean(data.enableSensory),
        };
      })
      .catch(error => {
        runtimeError = error.toString();
        log("error", "http", "Failed to save runtime config.", error);
      })
      .finally(() => {
        runtimeSaving = false;
      });
  }

  onMount(() => {
    if (hasAuthConfig()) {
      loadRuntimeConfig();
    }
  });

</script>

<div class="config-box">
  <div class="field">
    <label for="endpoint">API endpoint</label>
    <input id="endpoint" type="value" bind:value={config.endpoint} placeholder="Required" />
  </div>
  <div class="field">
    <label for="token">Auth token</label>
    <input id="token" type="value" bind:value={config.token} placeholder="Required" autocomplete="off" />
  </div>
  <div class="field">
    <label for="user">User name</label>
    <input id="user" type="value" bind:value={config.user} placeholder="Required" />
  </div>
  <div class="section">
    <div class="field">
      <label for="enableNotification">Notification</label>
      <div class="inline-field">
        <input id="enableNotification" type="checkbox" bind:checked={runtimeConfig.enableNotification} onchange={saveRuntimeConfig} disabled={runtimeSaving} />
        <span class="inline-label">Enable</span>
      </div>
    </div>
    <div class="field">
      <label for="enableSensory">Sensory</label>
      <div class="inline-field">
        <input id="enableSensory" type="checkbox" bind:checked={runtimeConfig.enableSensory} onchange={saveRuntimeConfig} disabled={runtimeSaving} />
        <span class="inline-label">Enable</span>
      </div>
    </div>
    {#if runtimeError !== ""}
      <div class="error-text">{runtimeError}</div>
    {/if}
  </div>
</div>

<style>

.config-box {
  font-size: 0.8rem;
  font-weight: 400;
  line-height: 2rem;
}

.field {
  display: flex;
  flex-direction: column;
}

.inline-field {
  flex-direction: row;
  align-items: center;
  line-height: 1.2rem;
}

.inline-label {
  margin: 0;
}

input {
  outline: none;
  border: none;
  font-size: 0.8rem;
}

.section {
  margin-top: 0.6rem;
  padding-top: 0.6rem;
  border-top: 1px solid #ddd;
}

.error-text {
  color: #c22;
  font-size: 0.7rem;
  line-height: 1rem;
  margin-top: 0.3rem;
}

</style>
