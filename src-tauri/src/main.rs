#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use calamine::{open_workbook_auto, DataType, Reader};
use csv::ReaderBuilder;
use serde::Serialize;
use std::fs::File;
use std::path::Path;
use tauri::{Emitter, Manager, Window};

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

    // Send headers first
    window
        .emit("parsed_headers", parsed.headers)
        .map_err(|e| format!("Failed to emit headers: {}", e))?;

    let batch_size = 500;
    let mut batch = Vec::new();

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

    Ok(ParsedFileResult { headers, rows })
}

fn parse_excel(filepath: String) -> Result<ParsedFileResult, String> {
    let mut workbook =
        open_workbook_auto(&filepath).map_err(|e| format!("Failed to open file: {}", e))?;

    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() {
        return Err("No sheets found.".into());
    }

    let range = workbook
        .worksheet_range(&sheet_names[0])
        .ok_or("Failed to find sheet.")?
        .map_err(|e| format!("Failed to read sheet: {}", e))?;

    let mut rows = Vec::new();
    let mut headers: Vec<String> = vec![];

    let mut iter = range.rows();
    if let Some(header_row) = iter.next() {
        headers = header_row
            .iter()
            .map(|cell| match cell {
                DataType::String(s) => s.clone(),
                DataType::Float(f) => f.to_string(),
                DataType::Int(i) => i.to_string(),
                DataType::Bool(b) => b.to_string(),
                _ => "".to_string(),
            })
            .collect();
    }

    for row in iter {
        let fields = row
            .iter()
            .map(|cell| match cell {
                DataType::String(s) => s.clone(),
                DataType::Float(f) => f.to_string(),
                DataType::Int(i) => i.to_string(),
                DataType::Bool(b) => b.to_string(),
                _ => "".to_string(),
            })
            .collect();
        rows.push(RowData { fields });
    }

    Ok(ParsedFileResult { headers, rows })
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
