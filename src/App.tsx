import { useState, useEffect, useRef } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Upload } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RowData } from "@/types/rowData";
import ReactECharts from "echarts-for-react";

export default function ExcelGraphApp() {
	const [headers, setHeaders] = useState<string[]>([]);
	const [fullData, setFullData] = useState<any[]>([]);
	const [selectedColumns, setSelectedColumns] = useState<string[]>([]);
	const [xAxisColumn, setXAxisColumn] = useState<string>("idx");
	const [loading, setLoading] = useState<boolean>(false);
	const [loadingPoints, setLoadingPoints] = useState<number>(0);
	const [totalPoints, setTotalPoints] = useState<number>(0);
	const [finishedLoading, setFinishedLoading] = useState<boolean>(false);

	const tempBuffer = useRef<any[]>([]);
	const flushing = useRef<boolean>(false);

	useEffect(() => {
		const unlistenHeaders = listen<string[]>("parsed_headers", (event) => {
			setHeaders(event.payload);
			setFullData([]);
			tempBuffer.current = [];
		});

		const unlistenTotalRows = listen<number>(
			"parsed_total_rows",
			(event) => {
				setTotalPoints(event.payload);
			}
		);

		const unlistenRows = listen<RowData[]>("parsed_rows_batch", (event) => {
			const newRows = event.payload.map((row) => {
				const obj: { [key: string]: any } = {};
				headers.forEach((header, i) => {
					const val = row.fields[i];
					obj[header] =
						val === undefined
							? null
							: isNaN(Number(val))
							? val
							: Number(val);
				});
				return obj;
			});
			tempBuffer.current.push(...newRows);

			if (!flushing.current) {
				startFlushing();
			}
		});

		return () => {
			unlistenHeaders.then((f) => f());
			unlistenTotalRows.then((f) => f());
			unlistenRows.then((f) => f());
		};
	}, [headers]);

	const startFlushing = () => {
		flushing.current = true;
		const flushInterval = setInterval(() => {
			if (tempBuffer.current.length === 0) {
				clearInterval(flushInterval);
				flushing.current = false;
				setFinishedLoading(true);
				return;
			}

			setFullData((prev) => {
				const chunk = tempBuffer.current.splice(0, 500);
				setLoadingPoints((prev) => prev + chunk.length);
				return [...prev, ...chunk];
			});
		}, 250);
	};

	const handleOpenFile = async () => {
		const selected = await open({
			multiple: false,
			filters: [
				{ name: "Excel or CSV", extensions: ["csv", "xlsx", "xls"] },
			],
		});

		if (typeof selected === "string") {
			setLoading(true);
			setFinishedLoading(false);
			setLoadingPoints(0);
			try {
				await invoke("parse_file_stream", { filepath: selected });
				setXAxisColumn("idx");
				setSelectedColumns([]);
			} catch (error) {
				console.error("Failed to load file:", error);
			} finally {
				setLoading(false);
			}
		}
	};

	const handleCheckboxChange = (col: string) => {
		setSelectedColumns((prev) =>
			prev.includes(col) ? prev.filter((c) => c !== col) : [...prev, col]
		);
	};

	const pastelColors = [
		"#AEC6CF",
		"#FFB347",
		"#B39EB5",
		"#77DD77",
		"#FF6961",
		"#FDFD96",
		"#CFCFC4",
		"#FFD1DC",
		"#B0E0E6",
		"#E6E6FA",
	];

	const getChartOptions = () => {
		return {
			backgroundColor: "transparent",
			animation: false,
			tooltip: {
				trigger: "axis",
				backgroundColor: "rgba(30,30,30,0.8)",
				borderColor: "transparent",
				textStyle: { color: "#fff" },
			},
			legend: {
				textStyle: { color: "#ccc" },
			},
			xAxis: {
				type: "category",
				data: fullData.map((row, idx) => row[xAxisColumn] ?? idx),
				axisLine: { lineStyle: { color: "#555" } },
				axisLabel: { color: "#ccc" },
				splitLine: {
					show: true,
					lineStyle: {
						color: "#ccc",
						type: "dashed",
					},
				},
			},
			yAxis: {
				type: "value",
				axisLine: { lineStyle: { color: "#555" } },
				axisLabel: { color: "#ccc" },
				splitLine: {
					show: true,
					lineStyle: {
						color: "#ccc",
						type: "dashed",
					},
				},
			},

			series: selectedColumns.map((col, idx) => ({
				name: col,
				type: "line",
				data: fullData.map((row) => row[col]),
				smooth: false,
				showSymbol: false,
				lineStyle: {
					width: 2,
					color: pastelColors[idx % pastelColors.length],
					opacity: finishedLoading ? 1 : 0.5,
				},
				itemStyle: {
					color: pastelColors[idx % pastelColors.length], // Tooltip markers match this
				},
				emphasis: {
					focus: "series",
				},
				progressive: 5000,
				progressiveThreshold: 10000,
			})),
		};
	};

	return (
		<div className="flex h-screen bg-gradient-to-br from-gray-900 to-gray-800 p-4 relative">
			{/* Sidebar */}
			<div className="w-1/4 p-4 backdrop-blur-md bg-white/10 rounded-2xl shadow-lg overflow-y-auto">
				<button
					onClick={handleOpenFile}
					className="w-full mb-6 p-2 bg-blue-600 hover:bg-blue-700 rounded-lg text-white font-semibold"
				>
					Upload File
				</button>

				

				{finishedLoading && (
					<>
          <div className="text-white mb-4 font-semibold">
					Select X-Axis:
				</div>
				<select
					className="w-full mb-6 p-2 rounded-lg bg-gray-700 text-white border border-gray-600"
					value={xAxisColumn}
					onChange={(e) => setXAxisColumn(e.target.value)}
				>
					<option value="idx">Index</option>
					{headers.map((col, idx) => (
						<option key={idx} value={col}>
							{col}
						</option>
					))}
				</select>
						<div className="text-white mb-2 font-semibold">
							Select Data Series:
						</div>
						{headers.map(
							(col, idx) =>
								col !== xAxisColumn && (
									<div
										key={idx}
										className="flex items-center mb-2"
									>
										<Checkbox
											checked={selectedColumns.includes(
												col
											)}
											onCheckedChange={() =>
												handleCheckboxChange(col)
											}
										/>
										<span className="text-white ml-2">
											{col}
										</span>
									</div>
								)
						)}
					</>
				)}
			</div>

			{/* Chart area */}
			<div className="w-3/4 p-4 relative">
				<Card className="h-full backdrop-blur-md bg-white/10 rounded-2xl shadow-lg">
					<CardContent className="h-full flex items-center justify-center">
						{fullData.length === 0 || loading ? (
							<div className="flex flex-col items-center justify-center text-gray-400 text-center">
								<Upload
									size={64}
									className="text-blue-400 mb-4"
								/>
								<p className="text-lg">
									{loading
										? "Loading file..."
										: "Upload an Excel or CSV file"}
								</p>
							</div>
						) : (
							<ReactECharts
								style={{ height: "100%", width: "100%" }}
								option={getChartOptions()}
								notMerge={true}
								lazyUpdate={true}
							/>
						)}
					</CardContent>
				</Card>

				{/* Progress bar */}
				{!finishedLoading && totalPoints > 0 && (
					<div className="absolute bottom-4 left-4 right-4">
						<div className="w-full h-2 bg-gray-700 rounded-full overflow-hidden">
							<div
								className="h-full bg-blue-500 transition-all duration-300"
								style={{
									width: `${
										(loadingPoints / totalPoints) * 100
									}%`,
								}}
							/>
						</div>
						<div className="text-center text-xs text-gray-400 mt-1">
							{Math.min(
								100,
								Number(
									(
										(loadingPoints / totalPoints) *
										100
									).toFixed(1)
								)
							)}
							% loaded
						</div>
					</div>
				)}
			</div>
		</div>
	);
}
