import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import ReactECharts from "echarts-for-react";
import { Upload } from "lucide-react";
import { useCallback, useState } from "react";

// --- New Data Contracts for On-Demand Loading ---

interface FileOverview {
	headers: string[];
	sheets?: string[] | null;
	approx_rows?: number | null;
}

interface ColumnChunk {
	column: string;
	offset: number;
	values: any[];
	done: boolean;
}

export default function ExcelGraphApp() {
	const [fileName, setFileName] = useState<string>("Upload a Data File to Begin");
	const [headers, setHeaders] = useState<string[]>([]);
	const [selectedColumns, setSelectedColumns] = useState<string[]>([]);
	const [xAxisColumn, setXAxisColumn] = useState<string>("idx");
	const [loading, setLoading] = useState<boolean>(false);
    const [error, setError] = useState<string | null>(null);

    // --- New State for On-Demand Loading ---
    const [columns, setColumns] = useState<Record<string, any[]>>({});
    const [rowCount, setRowCount] = useState<number | null>(null);
    const [currentFile, setCurrentFile] = useState<string | null>(null);
    const [currentSheet, setCurrentSheet] = useState<string | null>(null);

	const handleOpenFile = async () => {
		const selected = await open({
			multiple: false,
			filters: [{ name: "Excel or CSV", extensions: ["csv", "xlsx", "xls"] }],
		});

		if (typeof selected !== "string") {
			return; // User cancelled
		}

		setLoading(true);
        setError(null);
		const parts = selected.split(/[/\\]/);
		const name = parts[parts.length - 1];
		setFileName(name);

        // Reset all data state
        setHeaders([]);
        setSelectedColumns([]);
        setColumns({});
        setRowCount(null);
        setCurrentFile(selected);
        setCurrentSheet(null);
        setXAxisColumn("idx");

		try {
			// 1. Invoke the new overview command
			const overview = await invoke<FileOverview>("open_file_overview", {
				filepath: selected,
			});
            
            // 2. Update state with metadata
			setHeaders(overview.headers);
			setRowCount(overview.approx_rows ?? null);
            setCurrentSheet(overview.sheets?.[0] ?? null); // Default to first sheet for Excel

		} catch (err: any) {
			console.error("Failed to get file overview:", err);
            setError(typeof err === 'string' ? err : "An unknown error occurred during file inspection.");
            setFileName("Failed to load file. Please try again.");
		} finally {
			setLoading(false);
		}
	};

    // Helper to load a full column from the backend in chunks
    const ensureColumnLoaded = useCallback(async (filepath: string, col: string, sheet?: string | null) => {
        if (!filepath || columns[col]) return; // Already loaded or no file selected

        console.log(`Loading column: ${col}`);
        setLoading(true);
        setError(null);

        const CHUNK_SIZE = 50000;
        let offset = 0;
        let allValues: any[] = [];

        try {
            while (true) {
                const chunk = await invoke<ColumnChunk>("load_column_chunk", {
                    filepath,
                    column: col,
                    sheet: sheet ?? null,
                    offset,
                    limit: CHUNK_SIZE
                });

                allValues = allValues.concat(chunk.values);
                
                // Update state progressively for better UI feedback
                setColumns(prev => ({ ...prev, [col]: allValues }));

                if (chunk.done) {
                    break;
                }
                offset += chunk.values.length;
                await new Promise(r => setTimeout(r, 0)); // Yield to main thread
            }
            if (rowCount === null) {
                setRowCount(allValues.length);
            }
        } catch (err: any) {
            console.error(`Failed to load column ${col}:`, err);
            setError(`Failed to load column "${col}": ${err}`);
            // Rollback partial data on error
            setColumns(prev => {
                const newCols = {...prev};
                delete newCols[col];
                return newCols;
            });
        } finally {
            setLoading(false);
        }
    }, [columns, rowCount]);

	const handleCheckboxChange = async (col: string) => {
        const isSelecting = !selectedColumns.includes(col);
        
        // Update selection immediately for responsive UI
		setSelectedColumns((prev) =>
			isSelecting ? [...prev, col] : prev.filter((c) => c !== col)
		);

        if (isSelecting && currentFile && !columns[col]) {
            await ensureColumnLoaded(currentFile, col, currentSheet);
        }
	};

    const handleXAxisChange = async (col: string) => {
        setXAxisColumn(col);
        if (col !== "idx" && currentFile && !columns[col]) {
            await ensureColumnLoaded(currentFile, col, currentSheet);
        }
    };

	const pastelColors = [
		"#AEC6CF", "#FFB347", "#B39EB5", "#77DD77", "#FF6961",
		"#FDFD96", "#CFCFC4", "#FFD1DC", "#B0E0E6", "#E6E6FA",
	];

	const getChartOptions = () => ({
		backgroundColor: "transparent",
        animation: true,
		tooltip: {
			trigger: "axis",
			backgroundColor: "rgba(30,30,30,0.8)",
			borderColor: "transparent",
			textStyle: { color: "#fff" },
		},
		legend: {
			data: selectedColumns,
			textStyle: { color: "#ccc" },
		},
		xAxis: {
			type: "category",
			data: xAxisColumn === "idx" 
                ? (rowCount ? Array.from({ length: rowCount }, (_, i) => i + 1) : []) 
                : (columns[xAxisColumn] ?? []),
			axisLine: { lineStyle: { color: "#555" } },
			axisLabel: { color: "#ccc" },
			splitLine: { show: false },
		},
		yAxis: {
			type: "value",
			axisLine: { lineStyle: { color: "#555" } },
			axisLabel: { color: "#ccc" },
			splitLine: { show: true, lineStyle: { color: "rgba(204, 204, 204, 0.2)", type: "dashed" } },
		},
		series: selectedColumns.map((col, idx) => ({
			name: col,
			type: "line",
			data: columns[col] ?? [],
			smooth: false,
			showSymbol: false,
			lineStyle: { width: 2, color: pastelColors[idx % pastelColors.length] },
			itemStyle: { color: pastelColors[idx % pastelColors.length] },
			emphasis: { focus: "series" },
			progressive: 10000,
			progressiveThreshold: 20000,
		})),
		dataZoom: [
			{ type: 'inside', start: 0, end: 100 },
			{ start: 0, end: 100 },
		],
	});

	return (
		<div className="flex h-screen bg-gradient-to-br from-gray-900 to-gray-800 p-4 relative font-sans">
			{/* Sidebar */}
			<div className="w-1/4 min-w-[250px] p-4 backdrop-blur-md bg-white/10 rounded-2xl shadow-lg overflow-y-auto flex flex-col">
				<button
					onClick={handleOpenFile}
					disabled={loading}
					className="w-full mb-6 p-2 bg-blue-600 hover:bg-blue-700 rounded-lg text-white font-semibold transition-colors disabled:bg-gray-500 disabled:cursor-not-allowed"
				>
					<div className="flex items-center justify-center">
                        {loading && <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-white mr-2"></div>}
                        {loading ? 'Loading...' : 'Upload File'}
                    </div>
				</button>

				{headers.length > 0 && (
					<>
						<div className="text-white mb-4 font-semibold">Select X-Axis:</div>
						<select
							className="w-full mb-6 p-2 rounded-lg bg-gray-700 text-white border border-gray-600"
							value={xAxisColumn}
							onChange={(e) => handleXAxisChange(e.target.value)}
						>
							<option value="idx">(Row Index)</option>
							{headers.map((col, idx) => (
								<option key={idx} value={col}>{col}</option>
							))}
						</select>

						<div className="text-white mb-2 font-semibold">Select Data Series (Y-Axis):</div>
						<div className="flex-grow overflow-y-auto pr-2">
							{headers.map((col, idx) =>
								col !== xAxisColumn && (
									<div key={idx} className="flex items-center mb-2">
										<Checkbox
											id={`col-${idx}`}
											checked={selectedColumns.includes(col)}
											onCheckedChange={() => handleCheckboxChange(col)}
										/>
										<label htmlFor={`col-${idx}`} className="text-white ml-2 cursor-pointer">{col}</label>
									</div>
								)
							)}
						</div>
					</>
				)}
			</div>

			{/* Chart area */}
			<div className="w-3/4 p-4 pl-0 relative flex-1">
				<Card className="h-full backdrop-blur-md bg-white/10 rounded-2xl shadow-lg">
					<CardContent className="h-full flex flex-col p-4">
						<div className="text-xl font-bold text-center text-white mb-4 transition-all duration-300">
							{fileName}
						</div>
						<div className="flex-1 flex items-center justify-center min-h-0">
							{loading && headers.length === 0 ? ( // Initial loading screen
                                <div className="flex flex-col items-center text-gray-400">
                                    <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-400 mb-4"></div>
                                    <p className="text-lg">Processing, please wait...</p>
                                </div>
                            ) : error ? (
                                <div className="text-red-400 text-center">
                                    <p>An error occurred:</p>
                                    <p className="text-sm font-mono mt-2">{error}</p>
                                </div>
                            ) : headers.length === 0 ? (
								<div className="flex flex-col items-center justify-center text-gray-400 text-center">
									<Upload size={64} className="text-blue-400 mb-4" />
									<p className="text-lg">Upload an Excel or CSV file to begin</p>
								</div>
							) : (
								<ReactECharts
									style={{ height: "100%", width: "100%" }}
									option={getChartOptions()}
									notMerge={false} // Use notMerge: false to allow progressive data updates
									lazyUpdate={true}
								/>
							)}
						</div>
					</CardContent>
				</Card>
			</div>
		</div>
	);
}