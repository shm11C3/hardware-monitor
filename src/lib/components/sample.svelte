<script>
import { invoke } from "@tauri-apps/api/tauri";

const intervalSec = 1;

let cpuUsage = 0;

const getCpu = async () => {
	return await invoke("get_cpu");
}

setInterval(async () => {
  cpuUsage = await getCpu();
}, intervalSec * 1000);


</script>

<div>
  <p>CPU: {cpuUsage}%</p>
</div>
