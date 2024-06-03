<script>
import { invoke } from "@tauri-apps/api/tauri";

const intervalSec = 1;

let cpuUsage = 0;
let memoryUsage = 0;

const getCpuUsage = async () => {
	return await invoke("get_cpu_usage");
}

const getMemoryUsage = async () => {
  return await invoke("get_memory_usage");
}

setInterval(async () => {
  cpuUsage = await getCpuUsage();
  memoryUsage = await getMemoryUsage();
}, intervalSec * 1000);


</script>

<div>
  <p>CPU: {cpuUsage}%</p>
  <p>MEMORY: {memoryUsage}%</p>
</div>
