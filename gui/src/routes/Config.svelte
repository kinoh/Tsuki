<script lang="ts">

  let config: { endpoint: string, token: string, user: string } = $state(JSON.parse(localStorage.getItem("config") ?? "{}"));

  $effect(() => {
    localStorage.setItem("config", JSON.stringify(config));
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

input {
  outline: none;
  border: none;
  font-size: 0.8rem;
}

</style>
