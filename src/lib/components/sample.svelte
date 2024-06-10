<script lang="ts">
import { onMount } from 'svelte';
import { getCpuMemoryHistory, getCpuUsage, getCpuUsageHistory, getGpuUsage, getGpuUsageHistory, getMemoryUsage } from '../../services/hardwareService';

const intervalSec = 1;

let cpuUsage = 0;
let memoryUsage = 0;
let gpuUsage= 0

let cpuHistory: number[] = [];
let memoryHistory: number[] = [];
let gpuHistory: number[] = [];

onMount(async () => {
  setInterval(async () => {
  cpuUsage = await getCpuUsage();
  memoryUsage = await getMemoryUsage();

  cpuHistory= await getCpuUsageHistory(30);
  memoryHistory = await getCpuMemoryHistory(30);

  gpuUsage = await getGpuUsage();
  gpuHistory = await getGpuUsageHistory(30);

}, intervalSec * 1000);
});



</script>

<div>
  <p>CPU: {cpuUsage}%</p>
  <p>MEMORY: {memoryUsage}%</p>
  <p>GPU: {gpuUsage}%</p>

  <div>
    <h3>CPU History</h3>
    <p>Count: {cpuHistory.length}</p>
    <ul>
      {#each cpuHistory as item}
        <li>{item}</li>
      {/each}
    </ul>
    <h3>MEMORY History</h3>
    <p>Count: {memoryHistory.length}</p>
    <ul>
      {#each memoryHistory as item}
        <li>{item}</li>
      {/each}
    </ul>
    <h3>GPU History</h3>
    <p>Count: {gpuHistory.length}</p>
    <ul>
      {#each gpuHistory as item}
        <li>{item}</li>
      {/each}
    </ul>
  </div>
</div>
