<script lang="ts">

  let config: { endpoint: string, token: string, user: string } = $state(JSON.parse(localStorage.getItem("config") ?? "{}"));

  $effect(() => {
    localStorage.setItem("config", JSON.stringify(config));
  });

</script>

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

<style>

@font-face {
  font-display: block;
  font-family: "SourceHanSans";
  src: url("/src/assets/fonts/SourceHanSans-VF.ttf");
}

:root {
  background: RGB(234, 210, 240) !important;
  font-family: "SourceHanSans", sans-serif;
  font-size: 1rem;
  font-weight: 400;
  line-height: 2rem;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100vh;
}

.field {
  display: flex;
  flex-direction: column;
}

input {
  outline: none;
  border: none;
  font-size: 1rem;
}

</style>
