import DoughnutChart from "@/components/charts/DoughnutChart";

const Dashboard = () => {
  return (
    <div className="p-8">
      <DoughnutChart chartData={70} dataType={"temp"} hardType="cpu" />
    </div>
  );
};

export default Dashboard;
