<script lang="ts">

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

<div class="field">
  <label for="serverMetadata">Server metadata</label>
  <pre id="serverMetadata" class="license">{serverMetadata}</pre>
</div>

<style>

@font-face {
  font-display: block;
  font-family: "SourceHanSans";
  src: url("/fonts/SourceHanSans-VF.ttf");
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

.license {
  background-color: #eee;
  outline: none;
  border: none;
  font-size: 0.65rem;
  line-height: 0.8rem;
  padding: 0.2rem;
  width: 98%;
  height: 6rem;
  overflow: scroll;
  margin: 0;
}

</style>
