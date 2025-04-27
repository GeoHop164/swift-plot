import { useState, useEffect, useRef } from "react";
import { LineChart, Line, XAxis, YAxis, Tooltip, Legend, ResponsiveContainer, CartesianGrid } from "recharts";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Upload } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RowData } from "@/types/rowData";

export default function ExcelGraphApp() {
  const [headers, setHeaders] = useState<string[]>([]);
  const [data, setData] = useState<any[]>([]);
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
      setData([]);
      tempBuffer.current = [];
    });

    const unlistenTotalRows = listen<number>("parsed_total_rows", (event) => {
      setTotalPoints(event.payload);
    });

    const unlistenRows = listen<RowData[]>("parsed_rows_batch", (event) => {
      const newRows = event.payload.map((row) => {
        const obj: { [key: string]: any } = {};
        headers.forEach((header, i) => {
          const val = row.fields[i];
          obj[header] = val === undefined ? null : (isNaN(Number(val)) ? val : Number(val));
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
        return;
      }

      setData(prev => {
        const chunk = tempBuffer.current.splice(0, 500);
        return [...prev, ...chunk];
      });
    }, 250); 
  };

  const handleOpenFile = async () => {
    const selected = await open({
      multiple: false,
      filters: [
        { name: "Excel or CSV", extensions: ["csv", "xlsx", "xls"] }
      ]
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
    setSelectedColumns(prev =>
      prev.includes(col) ? prev.filter(c => c !== col) : [...prev, col]
    );
  };

  const pastelColors = [
    "#AEC6CF", "#FFB347", "#B39EB5", "#77DD77", "#FF6961",
    "#FDFD96", "#CFCFC4", "#FFD1DC", "#B0E0E6", "#E6E6FA"
  ];

  return (
    <div className="flex h-screen bg-gradient-to-br from-gray-900 to-gray-800 p-4">
      {/* Sidebar */}
      <div className="w-1/4 p-4 backdrop-blur-md bg-white/10 rounded-2xl shadow-lg overflow-y-auto">
        <button
          onClick={handleOpenFile}
          className="w-full mb-6 p-2 bg-blue-600 hover:bg-blue-700 rounded-lg text-white font-semibold"
        >
          Upload File
        </button>

        <div className="text-white mb-4 font-semibold">Select X-Axis:</div>
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

        <div className="text-white mb-2 font-semibold">Select Data Series:</div>
        {headers.map((col, idx) => (
          col !== xAxisColumn && (
            <div key={idx} className="flex items-center mb-2">
              <Checkbox
                checked={selectedColumns.includes(col)}
                onCheckedChange={() => handleCheckboxChange(col)}
              />
              <span className="text-white ml-2">{col}</span>
            </div>
          )
        ))}
      </div>

      {/* Chart area */}
      <div className="w-3/4 p-4">
        <Card className="h-full backdrop-blur-md bg-white/10 rounded-2xl shadow-lg">
          <CardContent className="h-full flex items-center justify-center">
            {data.length === 0 || loading ? (
              <div className="flex flex-col items-center justify-center text-gray-400 text-center">
                <Upload size={64} className="text-blue-400 mb-4" />
                <p className="text-lg">{loading ? "Loading file..." : "Upload an Excel or CSV file"}</p>
              </div>
            ) : (
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={data}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.1)" />
                  <XAxis dataKey={xAxisColumn} stroke="#ccc" />
                  <YAxis stroke="#ccc" />
                  <Tooltip
                    contentStyle={{ backgroundColor: "rgba(30,30,30,0.8)", borderRadius: "12px", border: "none" }}
                    labelStyle={{ color: "#fff" }}
                    itemStyle={{ color: "#fff" }}
                  />
                  <Legend wrapperStyle={{ color: "#fff" }} />
                  {selectedColumns.map((col, idx) => (
                    <Line
                      key={col}
                      type="linear"
                      dataKey={col}
                      stroke={pastelColors[idx % pastelColors.length]}
                      dot={{ r: 0 }}
                      activeDot={{ r: 1 }}
                      strokeWidth={1}
                      strokeOpacity={finishedLoading ? 1 : 0.5}
                    />
                  ))}
                </LineChart>
              </ResponsiveContainer>
            )}
          </CardContent>
        </Card>
      </div>
      {!finishedLoading && totalPoints > 0 && (
        <div className="absolute bottom-4 left-4 right-4">
          <div className="w-full h-2 bg-gray-700 rounded-full overflow-hidden">
            <div
              className="h-full bg-blue-500 transition-all duration-300"
              style={{ width: `${(loadingPoints / totalPoints) * 100}%` }}
            />
          </div>
          <div className="text-center text-xs text-gray-400 mt-1">
            {Math.min(100, Number(((loadingPoints / totalPoints) * 100).toFixed(1)))}% loaded
          </div>
        </div>
      )}

    </div>
  );
}

