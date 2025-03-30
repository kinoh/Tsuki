<script lang="ts">
  import { fetch } from '@tauri-apps/plugin-http';
  import { PUBLIC_USER_NAME, PUBLIC_WEB_HOST, PUBLIC_WEB_AUTH_TOKEN } from '$env/static/public';
  import {
    onResume,
    onPause,
  } from "tauri-plugin-app-events-api";

  const WEB_HOST = PUBLIC_WEB_HOST;
  const WEB_AUTH_TOKEN = PUBLIC_WEB_AUTH_TOKEN;
  const USER_NAME = PUBLIC_USER_NAME;

  let messages: { role: string; chat: any }[] = $state([]);
  let inputText: string = $state("");
  let inputPlaceholder: string = $state("Connecting...");
  let avatarExpression: "default" | "blink" = $state("default");
  let connection: WebSocket | null = null;
  let intervalId: number | null = null;

  function connect() {
    fetch(`https://${WEB_HOST}/messages`, {
      headers: {
        "Authorization": `Bearer ${WEB_AUTH_TOKEN}`,
      }
    })
      .then(response => response.json())
      .then(data => {
        messages = [...data.toReversed(), ...messages];
      });

    connection = new WebSocket(`wss://${WEB_HOST}/ws`);

    connection.onopen = function(event) {
      inputPlaceholder = "";
      if (connection !== null) {
        connection.send(`${USER_NAME}:${WEB_AUTH_TOKEN}`);
      }
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
  }

  function handleSubmit(event: Event) {
    event.preventDefault();
    if (inputText.length == 0) {
      return;
    }
    if (connection !== null) {
      messages.unshift({
        "role": "user",
        "chat": { "content": inputText },
      });
      connection.send(inputText);
      inputText = "";
    }
  }

  function blink() {
    console.log(`blink ${close}`);

    intervalId = setInterval(() => {
      if (Math.random() < 0.1) {
        avatarExpression = "blink";
        setTimeout(() => {
          avatarExpression = "default";
        }, 50);
      }
    }, 500);
  }

  blink();
  connect();

  onPause(() => {
    console.log("App pause");
    if (intervalId !== null) {
      clearInterval(intervalId);
      intervalId = null;
    }
    avatarExpression = "blink";
    connection?.close();
    connection = null;
  });
  onResume(() => {
    console.log("App resume");
    if (intervalId === null) {
      blink();
    }
    if (connection === null) {
      connect();
    }
  });
</script>

<main class="container">
  <div class="layout">
    <div class="avatar-box">
    	{#each ["default", "blink"] as item}
        <img data-tauri-drag-region alt="tsuki avatar" class={["avatar", avatarExpression == item ? "shown" : "hidden"]} src={`tsuki_${item}.png`} />
      {/each}
    </div>
    <div class="message-list">
      <form onsubmit={handleSubmit}>
        <input class="message user-message" type="text" bind:value={inputText} placeholder={inputPlaceholder} />
      </form>
    	{#each messages as item, i}
        {#if i < 10}
          <div class="message {item.role.toLowerCase()}-message">
            {item.chat.content}
          </div>
        {/if}
      {/each}
    </div>
  </div>
</main>

<style>

@font-face {
  font-display: block;
  font-family: "SourceHanSans";
  src: url("/src/assets/fonts/SourceHanSans-VF.ttf");
}

:root {
  background: rgba(0, 0, 0, 0) !important;
  font-family: "SourceHanSans", sans-serif;
  font-size: 1rem;
  font-weight: 500;
  line-height: 1.2rem;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100vh;
}

.container {
  margin: 0.8rem 0.5rem;
}

.layout {
  display: flex;
  flex-direction: row;
  justify-content: center;
  align-items: stretch;
  gap: 0.5rem;
  height: calc(100vh - 1.6rem);
}

.message-list {
  overflow: hidden;
  display: flex;
  flex-direction: column-reverse;
  flex: auto;
  min-width: 0;
  padding: 0 2vw;
  mask-image: linear-gradient(to bottom, transparent 0%, #000 10%, #000 100%);
}

.avatar-box {
  overflow: hidden;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
}

.avatar {
  object-fit: contain;
  max-width: 10rem;
  filter: drop-shadow(0 0 6px #7763b3);
}

.avatar.shown {
  display: block;
}

.avatar.hidden {
  display: none;
}

.message {
  color: #222;
  padding: 0.8rem 1.2rem;
  border: none;
  border-radius: 5px;
  overflow-wrap: break-word;
}

.assistant-message {
  background: RGBA(224, 217, 240, 0.9);
  margin: 0.4rem 1.5rem 0.4rem 0;
  /* box-shadow: 0 0 5px #334466; */
}

.user-message {
  background-color: RGBA(255, 255, 255, 0.9);
  margin: 0.4rem 0 0.4rem 1.5rem;
  transition: border-color 0.25s;
  /* box-shadow: 0 0 5px gray; */
}

form {
  margin-bottom: 1rem;
  display: flex;
  flex-direction: column;
}

.row {
  display: flex;
  justify-content: center;
}

input {
  outline: none;
  font-size: 1rem;
}

@media screen and (max-width: 36rem) {
  :root {
    background: #bbbbc3 !important;
  }

  .layout {
    flex-direction: column;
    justify-content: stretch;
  }

  .avatar-box {
    background-color: #f3f3f3;
    max-height: 15rem;
    border-radius: 6px;
  }

  .avatar {
    max-width: 12rem;
  }

  form {
    margin-bottom: 0;
  }
}

</style>
