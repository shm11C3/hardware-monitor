export type LegendItem = {
  label: string;
  icon: JSX.Element;
  datasetIndex: number;
};

const CustomLegend = ({
  item,
}: {
  item: LegendItem;
}) => {
  return (
    <div className="custom-legend">
      <div className="cursor-default flex items-center">
        {item.icon}
        <span className="ml-2">{item.label}</span>
      </div>
    </div>
  );
};

export default CustomLegend;
