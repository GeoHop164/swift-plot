import React, { useState } from 'react';
import { LineChart, Line, XAxis, YAxis, Tooltip, Legend, ResponsiveContainer, CartesianGrid } from 'recharts';
import { Card, CardContent } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Upload } from 'lucide-react';
import ExcelParserWorker from './parseExcel.worker.ts?worker';
import './App.css';
// import { invoke } from '@tauri-apps/api/tauri';
import { invoke } from '@tauri-apps/api/core';

// const sayHello = async () => {
//   const response = await invoke<string>('greet', { name: 'World' });
//   console.log(response);
// };

// sayHello();

export default function ExcelGraphApp() {
  const [data, setData] = useState<any[]>([]);
  const [columns, setColumns] = useState<string[]>([]);
  const [selectedColumns, setSelectedColumns] = useState<string[]>([]);
  const [xAxisColumn, setXAxisColumn] = useState<string>('idx');
  const [loading, setLoading] = useState<boolean>(false);
  const [dragActive, setDragActive] = useState<boolean>(false);

  const handleFile = async (e: React.ChangeEvent<HTMLInputElement> | DragEvent) => {
    e.preventDefault();
    let file: File | null = null;
    if ('dataTransfer' in e) {
      file = e.dataTransfer?.files?.[0] || null;
    } else if ('target' in e) {
      file = (e.target as HTMLInputElement).files?.[0] || null;
    }

    if (!file) return;

    setLoading(true); // Set loading to true before parsing

    const buffer = await file.arrayBuffer();
    const worker = new ExcelParserWorker();

    worker.postMessage({ fileBuffer: buffer });

    worker.onmessage = (event) => {
      const { jsonData } = event.data;
      setData(jsonData);
      setColumns(Object.keys(jsonData[0]));
      setSelectedColumns([]);
      setXAxisColumn('idx');
      worker.terminate();
      setLoading(false); // Set loading to false after parsing
    };
  };

  const handleCheckboxChange = (col: string) => {
    setSelectedColumns(prev =>
      prev.includes(col) ? prev.filter(c => c !== col) : [...prev, col]
    );
  };

  const pastelColors = [
    '#AEC6CF', '#FFB347', '#B39EB5', '#77DD77', '#FF6961',
    '#FDFD96', '#CFCFC4', '#FFD1DC', '#B0E0E6', '#E6E6FA'
  ];

  const onDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setDragActive(true);
  };

  const onDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    setDragActive(false);
  };

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragActive(false);
    handleFile(e.nativeEvent);
  };

  return (
    <div className="flex h-screen bg-gradient-to-br from-gray-900 to-gray-800 p-4">
      {/* Sidebar */}
      <div className="w-1/4 p-4 backdrop-blur-md bg-white/10 rounded-2xl shadow-lg overflow-y-auto">
        <input 
          type="file" 
          accept=".xlsx, .xls, .csv" 
          onChange={handleFile} 
          className="mb-6 w-full text-white"
        />

        <div className="text-white mb-4 font-semibold">Select X-Axis:</div>
        <select
          className="w-full mb-6 p-2 rounded-lg bg-gray-700 text-white border border-gray-600"
          value={xAxisColumn}
          onChange={(e) => setXAxisColumn(e.target.value)}
        >
          <option value="idx">Index</option>
          {columns.map((col, idx) => (
            <option key={idx} value={col}>
              {col}
            </option>
          ))}
        </select>

        <div className="text-white mb-2 font-semibold">Select Data Series:</div>
        {columns.filter(col => {
          if (!data.length) return false;
          const val = data[0][col];
          return typeof val === 'number' || !isNaN(parseFloat(val));
        }).map((col, idx) => (
          <div key={idx} className="flex items-center mb-2">
            <Checkbox
              checked={selectedColumns.includes(col)}
              onCheckedChange={() => handleCheckboxChange(col)}
            />
            <span className="text-white ml-2">{col}</span>
          </div>
        ))}
      </div>

      {/* Chart area */}
      <div className="w-3/4 p-4">
        <Card
          className={`h-full backdrop-blur-md bg-white/10 rounded-2xl shadow-lg ${dragActive ? 'border-2 border-blue-400' : ''}`}
          onDragOver={onDragOver}
          onDragLeave={onDragLeave}
          onDrop={onDrop}
        >
          <CardContent className="h-full flex items-center justify-center">
            {/* Show blank graph immediately */}
            {data.length === 0 || loading ? (
              <div className="flex flex-col items-center justify-center text-gray-400 text-center">
                <Upload size={64} className="text-blue-400 mb-4" />
                <p className="text-lg">Drag and drop your Excel file here</p>
                <p className="text-sm text-gray-500">or click the Upload button</p>
                {loading && <div className="mt-4 text-white">Loading data...</div>} {/* Loading indicator */}
              </div>
            ) : (
              <ResponsiveContainer width="100%" height="100%">
                <LineChart
                  data={data.map((d, idx) => ({
                    idx,
                    ...Object.fromEntries(
                      Object.entries(d).map(([k, v]) => [k, typeof v === 'number' ? v : parseFloat(v as any)])
                    )
                  }))}
                >
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.1)" />
                  <XAxis dataKey={xAxisColumn || 'idx'} stroke="#ccc" />
                  <YAxis stroke="#ccc" />
                  <Tooltip
                    contentStyle={{ backgroundColor: 'rgba(30,30,30,0.8)', borderRadius: '12px', border: 'none' }}
                    labelStyle={{ color: '#fff' }}
                    itemStyle={{ color: '#fff' }}
                  />
                  <Legend wrapperStyle={{ color: '#fff' }} />
                  {selectedColumns.map((col, idx) => (
                    <Line
                      key={col}
                      type="linear" // STRAIGHT LINES!
                      dataKey={col}
                      stroke={pastelColors[idx % pastelColors.length]}
                      dot={{ r: 0 }}
                      activeDot={{ r: 1 }}
                      strokeWidth={1}
                    />
                  ))}
                </LineChart>
              </ResponsiveContainer>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
