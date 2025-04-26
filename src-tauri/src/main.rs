#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use tauri::Manager;
use serde::Serialize;
use calamine::{open_workbook_auto, Reader, DataType};
use csv::ReaderBuilder;
use std::fs::File;
use std::path::Path;

// This struct will be returned to the frontend
#[derive(Serialize)]
struct RowData {
    fields: Vec<String>,
}

#[tauri::command]
async fn parse_file(filepath: String) -> Result<Vec<RowData>, String> {
    let path = Path::new(&filepath);

    if !path.exists() {
        return Err("File does not exist.".into());
    }

    let extension = path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    if extension == "csv" {
        parse_csv(filepath)
    } else if extension == "xlsx" || extension == "xls" {
        parse_excel(filepath)
    } else {
        Err("Unsupported file format.".into())
    }
}

fn parse_csv(filepath: String) -> Result<Vec<RowData>, String> {
    let file = File::open(filepath).map_err(|e| e.to_string())?;
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(file);

    let mut rows = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| e.to_string())?;
        let fields = record.iter().map(|s| s.to_string()).collect();
        rows.push(RowData { fields });
    }

    Ok(rows)
}

fn parse_excel(filepath: String) -> Result<Vec<RowData>, String> {
    let mut workbook = open_workbook_auto(&filepath)
        .map_err(|e| format!("Failed to open file: {}", e))?;

    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() {
        return Err("No sheets found.".into());
    }

    let range = workbook.worksheet_range(&sheet_names[0])
        .ok_or("Failed to find sheet.")?
        .map_err(|e| format!("Failed to read sheet: {}", e))?;

    let mut rows = Vec::new();

    for row in range.rows() {
        let fields = row.iter()
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

    Ok(rows)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![parse_file])
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
