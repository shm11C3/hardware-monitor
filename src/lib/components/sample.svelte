<script lang="ts">
import { onMount } from 'svelte';
import { getCpuMemoryHistory, getCpuUsage, getCpuUsageHistory, getMemoryUsage } from '../../services/hardwareService';

const intervalSec = 1;

let cpuUsage = 0;
let memoryUsage = 0;

let cpuHistory: number[] = [];
let memoryHistory: number[] = [];

onMount(async () => {
  setInterval(async () => {
  cpuUsage = await getCpuUsage();
  memoryUsage = await getMemoryUsage();

  cpuHistory= await getCpuUsageHistory(30);
  memoryHistory = await getCpuMemoryHistory(30);

}, intervalSec * 1000);
});



</script>

<div>
  <p>CPU: {cpuUsage}%</p>
  <p>MEMORY: {memoryUsage}%</p>

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
  </div>
</div>
