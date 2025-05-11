<script lang="ts">

  import { fetch } from '@tauri-apps/plugin-http';

  let config: { endpoint: string, token: string, user: string } = $state(JSON.parse(localStorage.getItem("config") ?? "{}"));
  let serverMetadata = $state("");

  function secure(): "s" | "" {
    return ((config.endpoint && config.endpoint.match(/^localhost|^10\.0\.2\.2/)) ? "" : "s");
  }

  fetch(`http${secure()}://${config.endpoint}/metadata`, {
      headers: {
        "Authorization": `Bearer ${config.token}`,
      }
    })
    .then(response => response.json())
    .then(json => {
      serverMetadata = JSON.stringify(json, null, "  ");
    })
    .catch(error => {
      console.log("fetch error: " + error);
    });

</script>

<div class="status-box">
  <div class="field">
    <label for="serverMetadata">Server metadata</label>
    <pre id="serverMetadata" class="metadata">{serverMetadata}</pre>
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

</style>
