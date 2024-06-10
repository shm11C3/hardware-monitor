<script lang="ts">
  import { CategoryScale, Legend, LineElement, LinearScale, PointElement, Title, Tooltip } from 'chart.js';
  import type { Chart as ChartJS,  ChartOptions, DefaultDataPoint  } from 'chart.js';
  import Chart from 'chart.js/auto';
  import { onMount } from 'svelte';
  import { Line } from 'svelte-chartjs';
  import { writable } from 'svelte/store';
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
        grid: {
          color: 'rgba(255, 255, 255, 0.2)', // y軸のグリッドの色を薄い白に設定
        },
        ticks: {
          color: '#fff', // y軸のラベルの色を白に設定
        },
      },
    },
    elements: {
      point: {
        radius: 0, // 点の半径を0にする
        hoverRadius: 0, // ホバー時の点の半径を0にする
      },
      line: {
        tension: 0.4, // 曲線の滑らかさを調整
      },
    },
    plugins: {
      legend: {
        display: true,
        labels: {
          color: '#fff', // 凡例のラベルの色を白に設定
        },
      },
      tooltip: {
        backgroundColor: 'rgba(0, 0, 0, 0.7)', // ツールチップの背景色を半透明の黒に設定
        titleColor: '#fff', // ツールチップのタイトルの色を白に設定
        bodyColor: '#fff', // ツールチップの本文の色を白に設定
      },
    },
  };

  // グラデーションの設定
  let cpuGradient: CanvasGradient;
  let memoryGradient: CanvasGradient;

  function setGradients(ctx: CanvasRenderingContext2D) {
    const gradient = ctx.createLinearGradient(0, 0, 0, ctx.canvas.height);
    gradient.addColorStop(0, 'rgba(75, 192, 192, 1)');
    gradient.addColorStop(1, 'rgba(75, 192, 192, 0.3)');
    cpuGradient = gradient;

    const gradient2 = ctx.createLinearGradient(0, 0, 0, ctx.canvas.height);
    gradient2.addColorStop(0, 'rgba(255, 99, 132, 1)');
    gradient2.addColorStop(1, 'rgba(255, 99, 132, 0.3)');
    memoryGradient = gradient2;
  }

  // Svelteのストアを使用して参照を保持
  const cpuChartRef = writable<ChartJS<'line'> | null>(null);
  const memoryChartRef = writable<ChartJS<'line'> | null>(null);

  // ストアの値が変わるたびにグラデーションを設定
  cpuChartRef.subscribe(chart => {
    if (chart) {
      setGradients(chart.ctx);
    }
  });

  memoryChartRef.subscribe(chart => {
    if (chart) {
      setGradients(chart.ctx);
    }
  });

  $: cpuChartData = {
    labels,
    datasets: [
      {
        label: 'CPU Usage (%)',
        data: cpuData,
        borderColor: 'rgb(75, 192, 192)',
        backgroundColor: cpuGradient,
        fill: true, // グラデーションを有効にする
        pointRadius: 0, // 点の半径を0にする
        pointHoverRadius: 0, // ホバー時の点の半径を0にする
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
        backgroundColor: memoryGradient,
        fill: true, // グラデーションを有効にする
        pointRadius: 0, // 点の半径を0にする
        pointHoverRadius: 0, // ホバー時の点の半径を0にする
      },
    ],
  };
</script>

<style>
  .chart-container {
    width: 100%;
    max-width: 600px;
    margin: 0 auto;
    background: #2c3e50; /* 背景色を設定 */
    padding: 20px;
    border-radius: 10px;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.2);
  }
</style>

<div class="chart-container">
  <Line data={cpuChartData} {options} />
  <Line data={memoryChartData} {options} />
</div>
