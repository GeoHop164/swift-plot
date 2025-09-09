#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use calamine::{open_workbook_auto, Data, Reader, Sheets};
use csv::ReaderBuilder;
use log::info;
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;
use tauri::Manager;

// --- Existing Structs (for reference, load_file_fully is kept but unused by UI) ---
#[derive(Serialize, Clone, Debug)]
struct RowData {
    fields: Vec<serde_json::Value>,
}

#[derive(Serialize, Clone, Debug)]
struct FullDataPayload {
    headers: Vec<String>,
    rows: Vec<RowData>,
    total_rows: usize,
}

// --- New Structs for On-Demand Loading ---

/// Overview of a file, containing metadata like headers and sheet names.
#[derive(Serialize, Clone, Debug)]
struct FileOverview {
    headers: Vec<String>,
    sheets: Option<Vec<String>>,
    approx_rows: Option<usize>,
}

/// A chunk of data for a single column.
#[derive(Serialize, Clone, Debug)]
struct ColumnChunk {
    column: String,
    offset: usize,
    values: Vec<serde_json::Value>,
    done: bool,
}

// --- New Tauri Commands for On-Demand Loading ---

/// Opens a file and returns its metadata without loading the full content.
#[tauri::command]
fn open_file_overview(filepath: String) -> Result<FileOverview, String> {
    let start_time = Instant::now();
    info!("Opening file overview for: {}", filepath);

    let path = Path::new(&filepath);
    if !path.exists() {
        return Err("File does not exist.".into());
    }

    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();

    let result = match extension.as_str() {
        "csv" => {
            let mut rdr = ReaderBuilder::new().has_headers(true).from_path(&filepath).map_err(|e| e.to_string())?;
            let headers = rdr.headers().map_err(|e| e.to_string())?.iter().map(String::from).collect();
            Ok(FileOverview {
                headers,
                sheets: None,
                approx_rows: None,
            })
        }
        "xlsx" | "xls" => {
            let mut workbook: Sheets<BufReader<File>> = open_workbook_auto(&filepath).map_err(|e| e.to_string())?;
            let sheet_names = workbook.sheet_names().to_owned();
            if sheet_names.is_empty() {
                return Err("No sheets found in the workbook.".into());
            }
            let first_sheet_name = &sheet_names[0];
            let range = workbook.worksheet_range(first_sheet_name).map_err(|e| e.to_string())?;
            let headers = range.rows().next()
                .map(|r| r.iter().map(excel_cell_to_string).collect())
                .unwrap_or_else(Vec::new);

            Ok(FileOverview {
                headers,
                sheets: Some(sheet_names),
                approx_rows: Some(range.height().saturating_sub(1)), // Subtract header row
            })
        }
        _ => Err("Unsupported file format.".into()),
    };

    info!("File overview completed in {:?}", start_time.elapsed());
    result
}

/// Loads a chunk of data for a specific column.
#[tauri::command]
fn load_column_chunk(filepath: String, column: String, sheet: Option<String>, offset: usize, limit: usize) -> Result<ColumnChunk, String> {
    let start_time = Instant::now();
    info!("Loading chunk for column '{}' in '{}' [offset: {}, limit: {}]", column, filepath, offset, limit);

    if limit == 0 {
        return Ok(ColumnChunk { column, offset, values: vec![], done: true });
    }

    let path = Path::new(&filepath);
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();

    let result = match extension.as_str() {
        "csv" => load_csv_column_chunk(&filepath, &column, offset, limit),
        "xlsx" | "xls" => load_excel_column_chunk(&filepath, &column, sheet, offset, limit),
        _ => Err("Unsupported file format.".into()),
    };
    
    info!("Column chunk loaded in {:?}", start_time.elapsed());
    result
}

// --- Helper Functions for Column Loading ---

fn load_csv_column_chunk(filepath: &str, column: &str, offset: usize, limit: usize) -> Result<ColumnChunk, String> {
    let mut rdr = ReaderBuilder::new().has_headers(true).from_path(filepath).map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?;
    let col_idx = headers.iter().position(|h| h == column)
        .ok_or_else(|| format!("Column '{}' not found in CSV.", column))?;

    let mut values = Vec::with_capacity(limit.min(10000)); // Cap initial capacity
    let mut records_iter = rdr.records().skip(offset);

    for _ in 0..limit {
        match records_iter.next() {
            Some(Ok(record)) => {
                let val_str = record.get(col_idx).unwrap_or("");
                
                let json_val = if val_str.is_empty() {
                    serde_json::Value::Null
                } else if let Ok(n) = val_str.trim().parse::<f64>() {
                    serde_json::Value::from(n)
                } else {
                    serde_json::Value::String(val_str.to_string())
                };
                values.push(json_val);
            }
            Some(Err(e)) => return Err(format!("CSV parsing error: {}", e)),
            None => break,
        }
    }

    let done = values.len() < limit;

    Ok(ColumnChunk {
        column: column.to_string(),
        offset,
        values,
        done,
    })
}

fn load_excel_column_chunk(filepath: &str, column: &str, sheet: Option<String>, offset: usize, limit: usize) -> Result<ColumnChunk, String> {
    let mut workbook: Sheets<BufReader<File>> = open_workbook_auto(filepath).map_err(|e| e.to_string())?;
    
    let sheet_name = sheet.or_else(|| workbook.sheet_names().get(0).cloned())
        .ok_or("No sheets found in workbook.")?;

    let range = workbook.worksheet_range(&sheet_name)
        .map_err(|e| format!("Could not read sheet '{}': {}", sheet_name, e))?;

    let headers = range.rows().next()
        .ok_or(format!("Sheet '{}' is empty.", sheet_name))?;
        
    let col_idx = headers.iter().position(|c| excel_cell_to_string(c) == column)
        .ok_or_else(|| format!("Column '{}' not found in sheet '{}'.", column, sheet_name))?;

    let values: Vec<serde_json::Value> = range.rows()
        .skip(1) // Skip header
        .skip(offset)
        .take(limit)
        .map(|row| {
            let cell = row.get(col_idx).unwrap_or(&Data::Empty);
            excel_cell_to_json(cell)
        })
        .collect();
    
    let done = values.len() < limit;

    Ok(ColumnChunk {
        column: column.to_string(),
        offset,
        values,
        done,
    })
}

// --- Existing Full Load Command (Unused by UI) ---
#[tauri::command]
async fn load_file_fully(filepath: String) -> Result<FullDataPayload, String> {
    let start_time = Instant::now();
    info!("(Legacy) Starting full file load for: {}", filepath);
    let path = Path::new(&filepath);
    if !path.exists() { return Err("File does not exist.".into()); }
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let result = if extension == "csv" { load_csv(&filepath) } else if extension == "xlsx" || extension == "xls" { load_excel(&filepath) } else { Err("Unsupported file format.".into()) };
    info!("(Legacy) Total processing time for {}: {:?}", filepath, start_time.elapsed());
    result
}

fn load_csv(filepath: &str) -> Result<FullDataPayload, String> {
    let mut reader = ReaderBuilder::new().has_headers(true).from_path(filepath).map_err(|e| e.to_string())?;
    let headers = reader.headers().map_err(|e| e.to_string())?.iter().map(String::from).collect::<Vec<String>>();
    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| e.to_string())?;
        let fields = record.iter().map(|s| serde_json::Value::String(s.to_string())).collect();
        rows.push(RowData { fields });
    }
    Ok(FullDataPayload { total_rows: rows.len(), headers, rows })
}

fn load_excel(filepath: &str) -> Result<FullDataPayload, String> {
    let mut workbook: Sheets<BufReader<File>> = open_workbook_auto(filepath).map_err(|e| format!("Failed to open file: {}", e))?;
    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() { return Err("No sheets found in the workbook.".into()); }
    let first_sheet_name = &sheet_names[0];
    let headers_range = workbook.worksheet_range(first_sheet_name).map_err(|e| format!("Error reading sheet '{}': {}", first_sheet_name, e))?;
    let headers = headers_range.rows().next().map(|r| r.iter().map(excel_cell_to_string).collect()).unwrap_or_else(Vec::new);
    if headers.is_empty() { return Err("Could not read headers from the first sheet.".into()); }
    let mut all_rows = Vec::new();
    for sheet_name in sheet_names.iter() {
        if let Ok(range) = workbook.worksheet_range(sheet_name) {
            all_rows.extend(range.rows().skip(1).map(|r| RowData { fields: r.iter().map(excel_cell_to_json).collect() }));
        }
    }
    Ok(FullDataPayload { total_rows: all_rows.len(), headers, rows: all_rows })
}

// --- Utility Functions ---
fn excel_cell_to_string(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.clone(),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DurationIso(s) | Data::DateTimeIso(s) => s.clone(),
        Data::Error(e) => format!("Error: {:?}", e),
        Data::Empty => String::new(),
    }
}

fn excel_cell_to_json(cell: &Data) -> serde_json::Value {
    match cell {
        Data::String(s) => serde_json::Value::String(s.clone()),
        Data::Float(f) => serde_json::Value::from(*f),
        Data::Int(i) => serde_json::Value::from(*i),
        Data::Bool(b) => serde_json::Value::from(*b),
        Data::DateTime(dt) => serde_json::Value::String(dt.to_string()),
        Data::DurationIso(s) | Data::DateTimeIso(s) => serde_json::Value::String(s.clone()),
        Data::Error(e) => serde_json::Value::String(format!("Error: {:?}", e)),
        Data::Empty => serde_json::Value::Null,
    }
}

// --- Main Application Setup ---
fn main() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            load_file_fully, // Kept for backward compatibility/reference
            open_file_overview,
            load_column_chunk
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            {
                if let Some(window) = app.get_webview_window("main") {
                    window.open_devtools();
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}