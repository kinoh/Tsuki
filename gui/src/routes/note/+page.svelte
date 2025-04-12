<script lang="ts">

  let sourceHansSansLicense = "";

  fetch("/src/assets/fonts/SourceHanSans-VF-LICENSE.txt")
    .then(response => response.text())
    .then(text => {
      sourceHansSansLicense = text;
    })
    .catch(error => {
      console.log("fetch error: " + error);
    });

</script>

<div class="field">
  <label for="sourceHansSansLicense">Source Han Sans license</label>
  <pre id="sourceHansSansLicense" class="license">
    {sourceHansSansLicense}
  </pre>
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
