import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import ReactECharts from "echarts-for-react";
import { Upload } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

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

type FilterOperator = "=" | "!=" | ">=" | ">" | "<=" | "<";

interface ColumnFilter {
	id: string;
	column: string;
	operator: FilterOperator;
	value: string;
}

const FILTER_OPERATORS: FilterOperator[] = ["=", "!=", ">=", ">", "<=", "<"];
const AUTO_SCALE_TARGET_FILL = 0.99;

const toNumberIfPossible = (value: any): number | null => {
	if (value === null || value === undefined) return null;
	const normalized = typeof value === "string" ? value.trim() : String(value);
	if (normalized.length === 0) return null;
	const parsed = Number(normalized);
	return Number.isFinite(parsed) ? parsed : null;
};

const matchesFilter = (cellValue: any, filter: ColumnFilter): boolean => {
	const numericCell = toNumberIfPossible(cellValue);
	const numericFilter = toNumberIfPossible(filter.value);
	const shouldUseNumeric = numericCell !== null && numericFilter !== null;

	const left = shouldUseNumeric ? numericCell : String(cellValue ?? "");
	const right = shouldUseNumeric ? numericFilter : filter.value;

	switch (filter.operator) {
		case "=":
			return left === right;
		case "!=":
			return left !== right;
		case ">=":
			return left >= right;
		case ">":
			return left > right;
		case "<=":
			return left <= right;
		case "<":
			return left < right;
		default:
			return false;
	}
};

const getSeriesBounds = (values: any[]) => {
	const numericValues = values
		.map((value) => toNumberIfPossible(value))
		.filter((value): value is number => value !== null);

	if (numericValues.length === 0) {
		return { min: 0, max: 1 };
	}

	let minValue = numericValues[0];
	let maxValue = numericValues[0];
	for (let i = 1; i < numericValues.length; i++) {
		const current = numericValues[i];
		if (current < minValue) minValue = current;
		if (current > maxValue) maxValue = current;
	}

	if (minValue === maxValue) {
		const padding = Math.max(Math.abs(minValue), 1) * 0.08;
		return {
			min: minValue - padding,
			max: maxValue + padding,
		};
	}

	const range = maxValue - minValue;
	const extraRange = range * ((1 / AUTO_SCALE_TARGET_FILL) - 1);
	const visualPadding = extraRange / 2;
	return {
		min: minValue - visualPadding,
		max: maxValue + visualPadding,
	};
};

const formatAxisTick = (value: any, maxSigFigs = 5) => {
	const numeric = toNumberIfPossible(value);
	if (numeric === null) return String(value ?? "");
	if (numeric === 0) return "0";

	const abs = Math.abs(numeric);
	if (abs >= 1e6 || abs < 1e-3) {
		return numeric.toExponential(maxSigFigs - 1)
			.replace(/\.?0+e/, "e")
			.replace("e+", "e");
	}

	const decimals = Math.max(0, Math.min(8, maxSigFigs - Math.floor(Math.log10(abs)) - 1));
	return numeric.toFixed(decimals).replace(/\.?0+$/, "");
};

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
	const [filters, setFilters] = useState<ColumnFilter[]>([]);
	const [filterColumn, setFilterColumn] = useState<string>("");
	const [filterOperator, setFilterOperator] = useState<FilterOperator>("=");
	const [filterValue, setFilterValue] = useState<string>("");
	const [autoScaleSeriesAxes, setAutoScaleSeriesAxes] = useState<boolean>(false);

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
	        setFilters([]);
	        setFilterColumn("");
	        setFilterOperator("=");
	        setFilterValue("");

		try {
			// 1. Invoke the new overview command
			const overview = await invoke<FileOverview>("open_file_overview", {
				filepath: selected,
			});
	            
	            // 2. Update state with metadata
				setHeaders(overview.headers);
				setRowCount(overview.approx_rows ?? null);
	            setCurrentSheet(overview.sheets?.[0] ?? null); // Default to first sheet for Excel
	            setFilterColumn(overview.headers[0] ?? "");

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

	const handleAddFilter = async () => {
		const value = filterValue.trim();
		if (!filterColumn || value.length === 0) {
			return;
		}

		if (currentFile && !columns[filterColumn]) {
			await ensureColumnLoaded(currentFile, filterColumn, currentSheet);
		}

		const newFilter: ColumnFilter = {
			id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
			column: filterColumn,
			operator: filterOperator,
			value,
		};

		setFilters((prev) => [...prev, newFilter]);
		setFilterValue("");
	};

	const handleRemoveFilter = (id: string) => {
		setFilters((prev) => prev.filter((filter) => filter.id !== id));
	};

    const handleXAxisChange = async (col: string) => {
        setXAxisColumn(col);
        if (col !== "idx" && currentFile && !columns[col]) {
            await ensureColumnLoaded(currentFile, col, currentSheet);
        }
    };

	const chartColors = [
		"#8FD3C4", "#F1B87A", "#E59A9F", "#A7C4E5", "#C7D89A",
		"#E2B6D2", "#90C8D7", "#D8A98F", "#ACCDB6", "#F0D8A8",
	];

	const checkboxClassName =
		"border-white/35 bg-white/5 data-[state=checked]:bg-[#A6D6CC] data-[state=checked]:border-[#BEE4DB] data-[state=checked]:text-slate-900 focus-visible:ring-[#BEE4DB]/40";

	const inferredRowCount = useMemo(() => {
		if (rowCount !== null) return rowCount;
		const lengths = Object.values(columns).map((vals) => vals.length);
		return lengths.length > 0 ? Math.max(...lengths) : 0;
	}, [columns, rowCount]);

	const filteredRowIndices = useMemo(() => {
		if (filters.length === 0) return null;

		const indices: number[] = [];
		for (let rowIdx = 0; rowIdx < inferredRowCount; rowIdx++) {
			const rowPasses = filters.every((filter) => {
				const columnValues = columns[filter.column];
				if (!columnValues) return false;
				return matchesFilter(columnValues[rowIdx], filter);
			});

			if (rowPasses) {
				indices.push(rowIdx);
			}
		}

		return indices;
	}, [columns, filters, inferredRowCount]);

	const xAxisData = useMemo(() => {
		if (xAxisColumn === "idx") {
			if (filteredRowIndices) {
				return filteredRowIndices.map((idx) => idx + 1);
			}
			return inferredRowCount > 0
				? Array.from({ length: inferredRowCount }, (_, i) => i + 1)
				: [];
		}

		const values = columns[xAxisColumn] ?? [];
		if (!filteredRowIndices) return values;
		return filteredRowIndices.map((idx) => values[idx]);
	}, [columns, filteredRowIndices, inferredRowCount, xAxisColumn]);

	const seriesWithData = useMemo(() => {
		return selectedColumns.map((col, idx) => {
			const values = columns[col] ?? [];
			const data = filteredRowIndices
				? filteredRowIndices.map((rowIdx) => values[rowIdx])
				: values;
			const bounds = getSeriesBounds(data);

			return {
				name: col,
				color: chartColors[idx % chartColors.length],
				data,
				min: bounds.min,
				max: bounds.max,
			};
		});
	}, [columns, filteredRowIndices, selectedColumns]);

	const multiAxisMode = autoScaleSeriesAxes && seriesWithData.length > 0;

	const yAxisConfig = useMemo(() => {
		if (!multiAxisMode) {
			return [
				{
					type: "value",
					scale: true,
					axisLine: { lineStyle: { color: "rgba(193, 211, 224, 0.4)" } },
					axisLabel: { color: "#c4d0dc", formatter: (value: number) => formatAxisTick(value) },
					splitLine: { show: true, lineStyle: { color: "rgba(193, 211, 224, 0.18)", type: "dashed" } },
				},
			];
		}

			return seriesWithData.map((series, idx) => {
				const axisOnLeft = idx % 2 === 0;
				const sideOffset = Math.floor(idx / 2) * 54;
					return {
						type: "value",
						scale: true,
						min: series.min,
						max: series.max,
						position: axisOnLeft ? "left" : "right",
						offset: sideOffset,
						axisLine: { show: true, lineStyle: { color: series.color, width: 1.4 } },
						axisLabel: { color: series.color, formatter: (value: number) => formatAxisTick(value) },
						splitLine: { show: idx === 0, lineStyle: { color: "rgba(193, 211, 224, 0.18)", type: "dashed" } },
				};
			});
		}, [multiAxisMode, seriesWithData]);

	const chartSeries = useMemo(() => {
		return seriesWithData.map((series, idx) => ({
			name: series.name,
			type: "line",
			data: series.data,
			yAxisIndex: multiAxisMode ? idx : 0,
			smooth: false,
			showSymbol: false,
			clip: true,
			lineStyle: { width: 2, color: series.color },
			itemStyle: { color: series.color },
			emphasis: { focus: "series" },
			progressive: 10000,
			progressiveThreshold: 20000,
		}));
	}, [multiAxisMode, seriesWithData]);

	const chartGrid = useMemo(() => {
		if (!multiAxisMode) {
			return { left: 66, right: 34, top: 64, bottom: 80 };
		}

		const leftAxes = Math.ceil(seriesWithData.length / 2);
		const rightAxes = Math.floor(seriesWithData.length / 2);
		const left = Math.min(240, 60 + Math.max(0, leftAxes - 1) * 54);
		const right = Math.min(240, 34 + rightAxes * 54);

		return { left, right, top: 70, bottom: 80 };
	}, [multiAxisMode, seriesWithData.length]);

	const getChartOptions = () => ({
		backgroundColor: "transparent",
		animation: true,
		tooltip: {
			trigger: "axis",
			backgroundColor: "rgba(14, 22, 31, 0.88)",
			borderWidth: 1,
			borderColor: "rgba(187, 209, 220, 0.34)",
			textStyle: { color: "#eaf2f8" },
		},
		legend: {
			data: selectedColumns,
			textStyle: { color: "#d6e2ec" },
		},
			xAxis: {
				type: "category",
				data: xAxisData,
				axisLine: { onZero: false, lineStyle: { color: "rgba(193, 211, 224, 0.4)" } },
				axisLabel: { color: "#c4d0dc" },
				splitLine: { show: false },
			},
			yAxis: yAxisConfig,
			grid: chartGrid,
			series: chartSeries,
		dataZoom: [
			{ type: "inside", start: 0, end: 100 },
			{
				start: 0,
				end: 100,
				borderColor: "rgba(193, 211, 224, 0.28)",
				backgroundColor: "rgba(25, 39, 53, 0.46)",
				fillerColor: "rgba(128, 171, 162, 0.34)",
				dataBackground: {
					lineStyle: { color: "rgba(153, 180, 199, 0.52)" },
					areaStyle: { color: "rgba(153, 180, 199, 0.16)" },
				},
				handleStyle: {
					color: "#9ccbc0",
					borderColor: "#d8ebe5",
				},
				textStyle: { color: "#c8d4df" },
			},
		],
	});

	return (
		<div className="app-shell">
			<div className="ambient-orb orb-1" />
			<div className="ambient-orb orb-2" />
			<div className="ambient-orb orb-3" />
			<div className="app-layout">
				<div className="glass-panel sidebar-panel">
					<button
						onClick={handleOpenFile}
						disabled={loading}
						className="btn btn-primary w-full"
					>
						<div className="flex items-center justify-center gap-2">
							{loading && <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-white"></div>}
							{loading ? "Loading..." : "Upload File"}
						</div>
					</button>

					{headers.length > 0 ? (
						<>
							<div className="section-title">Select X-Axis</div>
							<select
								className="control-select mb-6"
								value={xAxisColumn}
								onChange={(e) => handleXAxisChange(e.target.value)}
							>
								<option value="idx">(Row Index)</option>
								{headers.map((col, idx) => (
									<option key={idx} value={col}>{col}</option>
								))}
							</select>

								<div className="section-title">Select Data Series (Y-Axis)</div>
								<div className="series-scroll">
								{headers.map((col, idx) =>
									col !== xAxisColumn && (
										<div key={idx} className="series-item">
											<Checkbox
												id={`col-${idx}`}
												className={checkboxClassName}
												checked={selectedColumns.includes(col)}
												onCheckedChange={() => handleCheckboxChange(col)}
											/>
											<label htmlFor={`col-${idx}`} className="series-label">{col}</label>
										</div>
									)
									)}
								</div>

								<div className="axis-scale-toggle">
									<Checkbox
										id="auto-scale-axes"
										className={checkboxClassName}
										checked={autoScaleSeriesAxes}
										onCheckedChange={(checked) => setAutoScaleSeriesAxes(checked === true)}
									/>
									<label htmlFor="auto-scale-axes" className="axis-scale-label">
										Auto-scale each selected series (multi Y-axis)
									</label>
								</div>

								<div className="filter-editor">
									<div className="section-title">Add Filter</div>
								<select
									className="control-select mb-2"
									value={filterColumn}
									onChange={(e) => setFilterColumn(e.target.value)}
								>
									{headers.map((col, idx) => (
										<option key={`filter-col-${idx}`} value={col}>{col}</option>
									))}
								</select>
								<div className="control-row">
									<select
										className="control-select compact-select"
										value={filterOperator}
										onChange={(e) => setFilterOperator(e.target.value as FilterOperator)}
									>
										{FILTER_OPERATORS.map((op) => (
											<option key={op} value={op}>{op}</option>
										))}
									</select>
									<input
										className="control-input"
										placeholder="Value"
										value={filterValue}
										onChange={(e) => setFilterValue(e.target.value)}
									/>
								</div>
								<button
									className="btn btn-secondary w-full mt-2"
									onClick={handleAddFilter}
									disabled={!filterColumn || filterValue.trim().length === 0 || loading}
								>
									Add Filter
								</button>
							</div>

							<div className="sidebar-filters-dock">
								<div className="filters-title">Active Filters</div>
								{filters.length === 0 ? (
									<p className="filter-empty">No filters applied.</p>
								) : (
									<div className="sidebar-filters-scroll">
										{filters.map((filter) => (
											<div key={filter.id} className="filter-pill">
												<span className="filter-pill-text">
													{filter.column} {filter.operator} {filter.value}
												</span>
												<button
													type="button"
													className="filter-pill-remove"
													onClick={() => handleRemoveFilter(filter.id)}
													aria-label={`Remove filter ${filter.column} ${filter.operator} ${filter.value}`}
												>
													x
												</button>
											</div>
										))}
									</div>
								)}
							</div>
						</>
					) : (
						<p className="sidebar-hint">Upload a file to configure plotted series and filter rules.</p>
					)}
				</div>

				<div className="chart-workspace">
					<Card className="glass-panel chart-panel !gap-0 !py-0 !border-white/10 !bg-transparent !shadow-none">
						<CardContent className="h-full flex flex-col p-5 md:p-6">
							<div className="panel-title">{fileName}</div>
							<div className="chart-host">
								{loading && headers.length === 0 ? (
									<div className="loading-state">
										<div className="animate-spin rounded-full h-12 w-12 border-b-2 border-[#A8D3C9] mb-4"></div>
										<p className="text-lg">Processing, please wait...</p>
									</div>
								) : error ? (
									<div className="error-state">
										<p>An error occurred:</p>
										<p className="text-sm font-mono mt-2">{error}</p>
									</div>
								) : headers.length === 0 ? (
									<div className="empty-state">
										<Upload size={64} className="empty-icon mb-4" />
										<p className="text-lg">Upload an Excel or CSV file to begin</p>
									</div>
								) : (
									<ReactECharts
										style={{ height: "100%", width: "100%" }}
										option={getChartOptions()}
										// Force full option replacement so deselected series are removed.
										notMerge={true}
										lazyUpdate={true}
									/>
								)}
							</div>
						</CardContent>
					</Card>
				</div>
			</div>
		</div>
	);
}
