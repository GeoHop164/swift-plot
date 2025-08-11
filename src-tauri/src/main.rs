#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use calamine::{open_workbook_auto, Reader};
use csv::ReaderBuilder;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::fs::File;
use std::path::Path;
use tauri::{Manager, Window, Emitter};

#[derive(Serialize, Clone)]
struct RowData {
    fields: Vec<String>,
}

#[derive(Serialize)]
struct ParsedFileResult {
    headers: Vec<String>,
    rows: Vec<RowData>,
}

#[tauri::command]
async fn parse_file_stream(filepath: String, window: Window) -> Result<(), String> {
    let path = Path::new(&filepath);
    if !path.exists() {
        return Err("File does not exist.".into());
    }

    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let parsed = if extension == "csv" {
        parse_csv(filepath)?
    } else if extension == "xlsx" || extension == "xls" {
        parse_excel(filepath)?
    } else {
        return Err("Unsupported file format.".into());
    };

    window
        .emit("parsed_headers", parsed.headers.clone())
        .map_err(|e| format!("Failed to emit headers: {}", e))?;

    window
        .emit("parsed_total_rows", parsed.rows.len())
        .map_err(|e| format!("Failed to emit total rows: {}", e))?;

    let batch_size = 500;
    let mut batch = Vec::with_capacity(batch_size);
    let total_rows = parsed.rows.len();

    for (idx, row) in parsed.rows.into_iter().enumerate() {
        batch.push(row);
        if batch.len() >= batch_size || idx == total_rows - 1 {
            window
                .emit("parsed_rows_batch", batch.clone())
                .map_err(|e| format!("Failed to emit batch: {}", e))?;
            batch.clear();
        }
    }

    Ok(())
}

fn parse_csv(filepath: String) -> Result<ParsedFileResult, String> {
    let file = File::open(filepath).map_err(|e| e.to_string())?;
    let mut reader = ReaderBuilder::new().has_headers(true).from_reader(file);

    let headers = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>();

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| e.to_string())?;
        let fields = record.iter().map(|s| s.to_string()).collect();
        rows.push(RowData { fields });
    }

    let flags = detect_duration_columns(headers.len(), &rows);
    let rows = convert_duration_columns(rows, &flags);

    Ok(ParsedFileResult { headers, rows })
}


fn excel_cell_to_string_seconds(cell: &calamine::Data) -> String {
    match cell {
        calamine::Data::String(s) => s.clone(),
        calamine::Data::Float(f) => f.to_string(),
        calamine::Data::Int(i) => i.to_string(),
        calamine::Data::Bool(b) => b.to_string(),

        // ExcelDateTime -> seconds since Excel's 0-day (days * 86400)
        calamine::Data::DateTime(dt) => {
            let secs = dt.as_f64() * 86_400.0; // convert Excel serial days to seconds
            format!("{:.6}", secs)
        }

        // Optional: if you want to handle ISO8601 duration strings directly:
        calamine::Data::DurationIso(s) => s.clone(), // or parse to seconds if needed
        calamine::Data::DateTimeIso(s) => s.clone(), // likewise

        _ => String::new(),
    
    }}

fn parse_excel(filepath: String) -> Result<ParsedFileResult, String> {
    let mut workbook =
        open_workbook_auto(&filepath).map_err(|e| format!("Failed to open file: {}", e))?;
    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() {
        return Err("No sheets found.".into());
    }

    
    let range = workbook
        .worksheet_range(&sheet_names[0])
        .map_err(|e| format!("Failed to read sheet: {}", e))?;


    let mut rows = Vec::new();
    let mut headers: Vec<String> = vec![];
    let mut iter = range.rows();

    if let Some(header_row) = iter.next() {
        headers = header_row.iter().map(excel_cell_to_string_seconds).collect();
    }

    for row in iter {
        let fields = row.iter().map(excel_cell_to_string_seconds).collect();
        rows.push(RowData { fields });
    }

    let flags = detect_duration_columns(headers.len(), &rows);
    let rows = convert_duration_columns(rows, &flags);

    Ok(ParsedFileResult { headers, rows })
}


fn parse_duration_to_seconds(s: &str) -> Option<f64> {
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?ix) ^
                \s*
                (?: (?P<d>\d+)\s*[dD]\s+ )?        # '2d', '2D', optional
                (?P<h>\d{1,3}) \s* : \s*           # hours (1–3 digits)
                (?P<m>\d{2})   \s* : \s*
                (?P<s>\d{2})
                (?: \.(?P<ms>\d{1,3}) )?           # optional .ms
                \s* $
            ",
        )
        .unwrap()
    });

    if let Some(c) = RE.captures(s) {
        let d: f64  = c.name("d").and_then(|m| m.as_str().parse().ok()).unwrap_or(0.0);
        let h: f64  = c.name("h")?.as_str().parse().ok()?;
        let m: f64  = c.name("m")?.as_str().parse().ok()?;
        let s2: f64 = c.name("s")?.as_str().parse().ok()?;
        let ms: f64 = c.name("ms").and_then(|m| m.as_str().parse().ok()).unwrap_or(0.0);
        Some(d * 86_400.0 + h * 3_600.0 + m * 60.0 + s2 + ms / 1_000.0)
    } else {
        None   
    }
}

fn detect_duration_columns(headers_len: usize, rows: &[RowData]) -> Vec<bool> {
    const THRESHOLD: f64 = 0.80;
    let mut is_duration = vec![false; headers_len];

    for col in 0..headers_len {
        let mut non_empty = 0usize;
        let mut parsable = 0usize;

        for r in rows {
            if col >= r.fields.len() {
                continue;
            }
            let v = r.fields[col].trim();
            if v.is_empty() {
                continue;
            }
            non_empty += 1;
            if parse_duration_to_seconds(v).is_some() {
                parsable += 1;
            }
        }

        if non_empty > 0 && (parsable as f64) / (non_empty as f64) >= THRESHOLD {
            is_duration[col] = true;
        }
    }

    is_duration
}

fn convert_duration_columns(mut rows: Vec<RowData>, is_duration: &[bool]) -> Vec<RowData> {
    for r in &mut rows {
        for (col, conv) in is_duration.iter().enumerate() {
            if *conv && col < r.fields.len() {
                if let Some(sec) = parse_duration_to_seconds(&r.fields[col]) {
                    r.fields[col] = format!("{:.6}", sec);
                }
            }
        }
    }
    rows
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![parse_file_stream])
        .setup(|app| {
            #[cfg(debug_assertions)]
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
