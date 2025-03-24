<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
	import { onMount } from 'svelte';

  let messages: { role: string; chat: any }[] = $state([]);
  let inputText: string = $state("");
  let inputPlaceholder: string = $state("Connecting...");
  let avatarImage: string = $state("/tsuki_default.png");

  fetch("http://localhost:2953/messages")
    .then(response => response.json())
    .then(data => {
      messages = [...data.toReversed(), ...messages];
    });

  let connection = new WebSocket("ws://localhost:2953/ws");

  connection.onopen = function(event) {
    inputPlaceholder = "";
  }
  connection.onclose = function(event) {
    inputPlaceholder = "Connection closed!";
  }
  connection.onmessage = function(event) {
    messages.unshift({
      "role": "assistant",
      "chat": { "content": event.data },
    });
  };

  function handleSubmit(event: Event) {
    event.preventDefault();
    if (inputText.length == 0) {
      return;
    }
    messages.unshift({
      "role": "user",
      "chat": { "content": inputText },
    });
    connection.send("きの " + inputText);
    inputText = "";
  }

  function blink() {
    avatarImage = "/tsuki_blink.png";
    setTimeout(() => {
      avatarImage = "/tsuki_default.png";
    }, 100);
    let interval = 500 + 8000 * Math.random();
    setTimeout(() => {
      blink();
    }, interval);
  }

  onMount(() => {
    blink();
  });
</script>

<main class="container">
  <div class="horizontal">
    <div class="avatar-box">
      <img data-tauri-drag-region alt="tsuki avatar" class="avatar" src={avatarImage} />
    </div>
    <div class="vertical">
      <form onsubmit={handleSubmit}>
        <input class="message user-message" type="text" bind:value={inputText} placeholder={inputPlaceholder} />
      </form>
    	{#each messages as item, i}
        {#if i < 5}
          <div class="message {item.role.toLowerCase()}-message">
            {item.chat.content}
          </div>
        {/if}
      {/each}
    </div>
  </div>
</main>

<style>

:root {
  background: rgba(0, 0, 0, 0) !important;
  font-family: "Meiryo", "Noto Sans", "Segoe UI", "Arial", sans-serif;
  font-size: 4mm;
  line-height: 1.2em;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100vh;
}

.container {
  margin: 0;
}

.horizontal {
  display: flex;
  flex-direction: row;
  gap: 5mm;
  height: 90vh;
}

.vertical {
  display: flex;
  flex-direction: column-reverse;
  flex: auto;
  margin: 2vh;
  min-width: 0;
}

.avatar-box {
  overflow: hidden;
  flex-shrink: 0;
}

.avatar {
  object-fit: contain;
  max-width: 20vw;
  filter: drop-shadow(0 0 8px #223344);
}

.message {
  color: #0f0f0f;
  padding: 0.8em 1.2em;
  border: none;
  border-radius: 5px;
  overflow-wrap: break-word;
}

.assistant-message {
  background: RGBA(224, 217, 240, 0.8);
  margin: 0.4em 1.5em 0.4em 0;
  box-shadow: 0 0 5px #334466;
}

.user-message {
  background-color: RGBA(255, 255, 255, 0.8);
  margin: 0.4em 0 0.4em 1.5em;
  transition: border-color 0.25s;
  box-shadow: 0 0 5px gray;
}

form {
  margin-top: auto;
  margin-bottom: 1em;
}

.row {
  display: flex;
  justify-content: center;
}

a {
  font-weight: 500;
  color: #646cff;
  text-decoration: inherit;
}

a:hover {
  color: #535bf2;
}

form {
  display: flex;
  flex-direction: column;
}

input,
button {
  font-size: 4mm;
}

button {
  cursor: pointer;
}

input,
button {
  outline: none;
}

</style>
