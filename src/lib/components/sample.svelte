<script>
import { invoke } from "@tauri-apps/api/tauri";

// biome-ignore lint/style/useConst: <explanation>
let name = "";
let greetMsg = "";

async function getCpu() {
	greetMsg = await invoke("get_cpu", { name });
}
</script>

<div>
  <input id="greet-input" placeholder="Enter a name..." bind:value="{name}" />
  <button on:click="{getCpu}">Greet</button>
  <p>{greetMsg}</p>
</div>
