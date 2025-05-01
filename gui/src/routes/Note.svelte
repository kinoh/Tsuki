<script lang="ts">

  let sourceHansSansLicense = "";

  fetch("/fonts/SourceHanSans-VF-LICENSE.txt")
    .then(response => response.text())
    .then(text => {
      sourceHansSansLicense = text;
    })
    .catch(error => {
      console.log("fetch error: " + error);
    });

</script>

<div class="note-box">
  <div class="field">
    <label for="sourceHansSansLicense">Source Han Sans license</label>
    <pre id="sourceHansSansLicense" class="license">{sourceHansSansLicense}</pre>
  </div>
</div>

<style>

.note-box {
  font-size: 0.8rem;
  font-weight: 400;
  line-height: 2rem;
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
  height: 10rem;
  overflow: scroll;
  margin: 0;
}

</style>
