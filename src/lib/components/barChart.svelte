<script lang="ts">
  import { CategoryScale, Legend, LineElement, LinearScale, PointElement, Title, Tooltip } from 'chart.js';
  import type { ChartOptions } from 'chart.js';
  import Chart from 'chart.js/auto';
  import { onMount } from 'svelte';
  import { Line } from 'svelte-chartjs';
  import { getCpuMemoryHistory, getCpuUsageHistory } from '../../services/hardwareService';

  Chart.register(CategoryScale, LinearScale, PointElement, LineElement, Title, Tooltip, Legend);

  let cpuData: number[]  = [];
  let memoryData: number[]  = [];
  let labels: string[] = [];

  async function fetchData() {
    const seconds = 60; // 過去60秒のデータを取得
    const newCpuData  = await getCpuUsageHistory(seconds);
    const newMemoryData  = await getCpuMemoryHistory(seconds);

    if (cpuData.length < seconds) {
      // 初期データを順次追加
      cpuData = [...cpuData, newCpuData[newCpuData.length - 1]];
      memoryData = [...memoryData, newMemoryData[newMemoryData.length - 1]];
      labels = [...labels, ""];
    } else {
      // 最大データ数に達したらシフト
      cpuData = [...cpuData.slice(1), newCpuData[newCpuData.length - 1]];
      memoryData = [...memoryData.slice(1), newMemoryData[newMemoryData.length - 1]];
      labels = [...labels.slice(1), ""];
    }
  }

  onMount(async () => {
    await fetchData();
    setInterval(fetchData, 1000); // 1秒ごとにデータを更新
  });

  const options: ChartOptions<'line'> = {
    responsive: true,
    animation: false,
    scales: {
      x: {
        display: false,
      },
      y: {
        display: true,
        suggestedMin: 0,
        suggestedMax: 100,
      },
    },
  };

  $: cpuChartData = {
    labels,
    datasets: [
      {
        label: 'CPU Usage (%)',
        data: cpuData,
        borderColor: 'rgb(75, 192, 192)',
        fill: false,
        pointRadius: 0,
        pointHoverRadius: 0,
      },
    ],
  };

  $: memoryChartData = {
    labels,
    datasets: [
      {
        label: 'Memory Usage (%)',
        data: memoryData,
        borderColor: 'rgb(255, 99, 132)',
        fill: false,
        pointRadius: 0,
        pointHoverRadius: 0,
      },
    ],
  };
</script>

<style>
  .chart-container {
    width: 100%;
    max-width: 600px;
    margin: 0 auto;
  }
</style>

<div class="chart-container">
  <Line data={cpuChartData} {options} />
  <Line data={memoryChartData} {options} />
</div>
