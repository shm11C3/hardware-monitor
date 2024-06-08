<script lang="ts">
import Chart from "../lib/components/lineChart.svelte";
import TestTemplate from "../lib/components/sample.svelte";

type ButtonState = 'chart' | 'raw'

let buttonState: ButtonState = 'chart';

const handleShowData = () => {
  buttonState = buttonState === "raw" ? "chart" : "raw";
};

</script>

<div>
  <h1>Hardware Monitor Proto</h1>
  <div>
    <button on:click={handleShowData}>{buttonState === "chart" ? "Show Raw Data" : "Show Char Sample"}</button>
  </div>
  {#if buttonState === "raw"}
    <TestTemplate />
  {:else}
    <Chart />
  {/if}
</div>
