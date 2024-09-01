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
			<div
				className="legend-item"
				style={{ cursor: "pointer", display: "flex", alignItems: "center" }}
			>
				{item.icon}
				<span style={{ marginLeft: "8px" }}>{item.label}</span>
			</div>
		</div>
	);
};

export default CustomLegend;
