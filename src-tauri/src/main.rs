#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use calamine::{open_workbook_auto, Data, Reader, Sheets};
use csv::ReaderBuilder;
use log::info;
use quick_xml::{
    events::{BytesStart, Event},
    name::QName,
    Reader as XmlReader,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::time::Instant;
use tauri::Manager;
use zip::ZipArchive;

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

#[derive(Clone, Copy, Debug)]
struct XlsxDimensions {
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
}

#[derive(Clone, Debug)]
struct XlsxWorkbookInfo {
    sheet_names: Vec<String>,
    sheet_paths: HashMap<String, String>,
}

#[derive(Clone, Debug)]
enum XlsxCellValue {
    Json(serde_json::Value),
    SharedString(usize),
}

// --- New Tauri Commands for On-Demand Loading ---

/// Opens a file and returns its metadata without loading the full content.
#[tauri::command]
async fn open_file_overview(
    filepath: String,
    skip_first_row: bool,
) -> Result<FileOverview, String> {
    tauri::async_runtime::spawn_blocking(move || open_file_overview_impl(filepath, skip_first_row))
        .await
        .map_err(|e| e.to_string())?
}

fn open_file_overview_impl(filepath: String, skip_first_row: bool) -> Result<FileOverview, String> {
    let start_time = Instant::now();
    info!(
        "Opening file overview for: {} [skip_first_row: {}]",
        filepath, skip_first_row
    );

    let path = Path::new(&filepath);
    if !path.exists() {
        return Err("File does not exist.".into());
    }

    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let result = match extension.as_str() {
        "csv" => open_csv_overview(&filepath, skip_first_row),
        "xlsx" => open_xlsx_overview_streaming(&filepath, skip_first_row),
        "xls" => open_excel_overview_range(&filepath, skip_first_row),
        _ => Err("Unsupported file format.".into()),
    };

    info!("File overview completed in {:?}", start_time.elapsed());
    result
}

/// Loads a chunk of data for a specific column.
#[tauri::command]
async fn load_column_chunk(
    filepath: String,
    column: String,
    sheet: Option<String>,
    offset: usize,
    limit: usize,
    skip_first_row: bool,
) -> Result<ColumnChunk, String> {
    tauri::async_runtime::spawn_blocking(move || {
        load_column_chunk_impl(filepath, column, sheet, offset, limit, skip_first_row)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn load_column_chunk_impl(
    filepath: String,
    column: String,
    sheet: Option<String>,
    offset: usize,
    limit: usize,
    skip_first_row: bool,
) -> Result<ColumnChunk, String> {
    let start_time = Instant::now();
    info!(
        "Loading chunk for column '{}' in '{}' [offset: {}, limit: {}]",
        column, filepath, offset, limit
    );

    if limit == 0 {
        return Ok(ColumnChunk {
            column,
            offset,
            values: vec![],
            done: true,
        });
    }

    let path = Path::new(&filepath);
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let result = match extension.as_str() {
        "csv" => load_csv_column_chunk(&filepath, &column, offset, limit, skip_first_row),
        "xlsx" | "xls" => {
            load_excel_column_chunk(&filepath, &column, sheet, offset, limit, skip_first_row)
        }
        _ => Err("Unsupported file format.".into()),
    };

    info!("Column chunk loaded in {:?}", start_time.elapsed());
    result
}

fn open_csv_overview(filepath: &str, skip_first_row: bool) -> Result<FileOverview, String> {
    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .from_path(filepath)
        .map_err(|e| e.to_string())?;
    let mut records = rdr.records();

    if skip_first_row {
        records.next().transpose().map_err(|e| e.to_string())?;
    }

    let headers = records
        .next()
        .transpose()
        .map_err(|e| e.to_string())?
        .ok_or("CSV file does not contain a header row.")?
        .iter()
        .map(String::from)
        .collect();

    Ok(FileOverview {
        headers,
        sheets: None,
        approx_rows: None,
    })
}

fn open_xlsx_overview_streaming(
    filepath: &str,
    skip_first_row: bool,
) -> Result<FileOverview, String> {
    let info = xlsx_workbook_info(filepath)?;
    let sheet_name = info
        .sheet_names
        .get(0)
        .ok_or("No sheets found in the workbook.")?;
    let sheet_path = info
        .sheet_paths
        .get(sheet_name)
        .ok_or_else(|| format!("Could not find sheet XML for '{}'.", sheet_name))?;
    let mut zip = open_xlsx_zip(filepath)?;
    let (headers, dimensions) = read_xlsx_headers_fast(&mut zip, sheet_path, skip_first_row)?;
    let approx_rows = dimensions
        .end_row
        .saturating_add(1)
        .saturating_sub(dimensions.start_row + header_row_index(skip_first_row) + 1);

    Ok(FileOverview {
        headers,
        sheets: Some(info.sheet_names),
        approx_rows: if approx_rows == 0 {
            None
        } else {
            Some(approx_rows)
        },
    })
}

fn open_excel_overview_range(filepath: &str, skip_first_row: bool) -> Result<FileOverview, String> {
    let mut workbook: Sheets<BufReader<File>> =
        open_workbook_auto(filepath).map_err(|e| e.to_string())?;
    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() {
        return Err("No sheets found in the workbook.".into());
    }
    let first_sheet_name = &sheet_names[0];
    let range = workbook
        .worksheet_range(first_sheet_name)
        .map_err(|e| e.to_string())?;
    let header_row_idx = header_row_index(skip_first_row);
    let headers = range
        .rows()
        .nth(header_row_idx)
        .map(|r| r.iter().map(excel_cell_to_string).collect())
        .unwrap_or_else(Vec::new);

    Ok(FileOverview {
        headers,
        sheets: Some(sheet_names),
        approx_rows: Some(range.height().saturating_sub(header_row_idx + 1)),
    })
}

// --- Helper Functions for Column Loading ---

fn load_csv_column_chunk(
    filepath: &str,
    column: &str,
    offset: usize,
    limit: usize,
    skip_first_row: bool,
) -> Result<ColumnChunk, String> {
    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .from_path(filepath)
        .map_err(|e| e.to_string())?;
    let mut records_iter = rdr.records();

    if skip_first_row {
        records_iter.next().transpose().map_err(|e| e.to_string())?;
    }

    let headers = records_iter
        .next()
        .transpose()
        .map_err(|e| e.to_string())?
        .ok_or("CSV file does not contain a header row.")?;
    let col_idx = headers
        .iter()
        .position(|h| h == column)
        .ok_or_else(|| format!("Column '{}' not found in CSV.", column))?;

    let mut values = Vec::with_capacity(limit.min(10000)); // Cap initial capacity
    let mut records_iter = records_iter.skip(offset);

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

fn load_excel_column_chunk(
    filepath: &str,
    column: &str,
    sheet: Option<String>,
    offset: usize,
    limit: usize,
    skip_first_row: bool,
) -> Result<ColumnChunk, String> {
    let mut workbook: Sheets<BufReader<File>> =
        open_workbook_auto(filepath).map_err(|e| e.to_string())?;

    let sheet_name = sheet
        .or_else(|| workbook.sheet_names().get(0).cloned())
        .ok_or("No sheets found in workbook.")?;

    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| format!("Could not read sheet '{}': {}", sheet_name, e))?;

    let header_row_idx = header_row_index(skip_first_row);
    let headers = range
        .rows()
        .nth(header_row_idx)
        .ok_or(format!("Sheet '{}' is empty.", sheet_name))?;

    let col_idx = headers
        .iter()
        .position(|c| excel_cell_to_string(c) == column)
        .ok_or_else(|| format!("Column '{}' not found in sheet '{}'.", column, sheet_name))?;

    let values: Vec<serde_json::Value> = range
        .rows()
        .skip(header_row_idx + 1)
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

fn open_xlsx_zip(filepath: &str) -> Result<ZipArchive<BufReader<File>>, String> {
    let file = File::open(filepath).map_err(|e| e.to_string())?;
    ZipArchive::new(BufReader::new(file)).map_err(|e| e.to_string())
}

fn xlsx_workbook_info(filepath: &str) -> Result<XlsxWorkbookInfo, String> {
    let mut zip = open_xlsx_zip(filepath)?;
    let relationships = read_xlsx_relationships(&mut zip)?;
    let (sheet_names, sheet_paths) = read_xlsx_workbook(&mut zip, &relationships)?;

    Ok(XlsxWorkbookInfo {
        sheet_names,
        sheet_paths,
    })
}

fn read_xlsx_relationships(
    zip: &mut ZipArchive<BufReader<File>>,
) -> Result<HashMap<String, String>, String> {
    let file = zip
        .by_name("xl/_rels/workbook.xml.rels")
        .map_err(|e| e.to_string())?;
    let mut xml = configured_xml_reader(file);
    let mut buf = Vec::with_capacity(1024);
    let mut relationships = HashMap::new();

    loop {
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                let id = xml_attr_value(&xml, &e, b"Id")?;
                let target = xml_attr_value(&xml, &e, b"Target")?;
                if let (Some(id), Some(target)) = (id, target) {
                    relationships.insert(id, normalize_xlsx_part_path(&target));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(relationships)
}

fn read_xlsx_workbook(
    zip: &mut ZipArchive<BufReader<File>>,
    relationships: &HashMap<String, String>,
) -> Result<(Vec<String>, HashMap<String, String>), String> {
    let file = zip.by_name("xl/workbook.xml").map_err(|e| e.to_string())?;
    let mut xml = configured_xml_reader(file);
    let mut buf = Vec::with_capacity(1024);
    let mut sheet_names = Vec::new();
    let mut sheet_paths = HashMap::new();

    loop {
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sheet" => {
                let name = xml_attr_value(&xml, &e, b"name")?;
                let rel_id = xml_attr_value(&xml, &e, b"r:id")?
                    .or_else(|| xml_attr_value(&xml, &e, b"relationships:id").ok().flatten());
                if let (Some(name), Some(rel_id)) = (name, rel_id) {
                    if let Some(path) = relationships.get(&rel_id) {
                        sheet_names.push(name.clone());
                        sheet_paths.insert(name, path.clone());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok((sheet_names, sheet_paths))
}

fn read_xlsx_headers_fast(
    zip: &mut ZipArchive<BufReader<File>>,
    sheet_path: &str,
    skip_first_row: bool,
) -> Result<(Vec<String>, XlsxDimensions), String> {
    let mut dimensions = XlsxDimensions {
        start_row: 0,
        start_col: 0,
        end_row: 0,
        end_col: 0,
    };
    let mut raw_headers = Vec::new();
    let mut header_row = header_row_index(skip_first_row);
    let mut has_dimensions = false;
    let mut saw_first_row = false;

    {
        let file = zip.by_name(sheet_path).map_err(|e| e.to_string())?;
        let mut xml = configured_xml_reader(file);
        let mut buf = Vec::with_capacity(4096);
        let mut skip_buf = Vec::with_capacity(4096);
        let mut fallback_row = 0usize;

        loop {
            buf.clear();
            match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
                Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"dimension" => {
                    if let Some(reference) = xml_attr_value(&xml, &e, b"ref")? {
                        if let Some(parsed) = parse_xlsx_dimension(&reference) {
                            dimensions = parsed;
                            has_dimensions = true;
                            header_row = dimensions.start_row + header_row_index(skip_first_row);
                            let width = dimensions
                                .end_col
                                .saturating_sub(dimensions.start_col)
                                .saturating_add(1);
                            raw_headers = vec![None; width];
                        }
                    }
                }
                Event::Start(e) if e.local_name().as_ref() == b"row" => {
                    let row = xml_attr_value(&xml, &e, b"r")?
                        .and_then(|value| value.parse::<usize>().ok())
                        .map(|value| value.saturating_sub(1))
                        .unwrap_or(fallback_row);
                    fallback_row = row.saturating_add(1);

                    if !has_dimensions && !saw_first_row {
                        dimensions.start_row = row;
                        dimensions.end_row = row;
                        header_row = row + header_row_index(skip_first_row);
                        saw_first_row = true;
                    }
                    dimensions.start_row = dimensions.start_row.min(row);
                    dimensions.end_row = dimensions.end_row.max(row);

                    if row == header_row {
                        read_xlsx_header_row(&mut xml, &mut raw_headers, dimensions.start_col)?;
                        break;
                    }

                    xml.read_to_end_into(QName(b"row"), &mut skip_buf)
                        .map_err(|e| e.to_string())?;

                    if row > header_row {
                        break;
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }

    let needed_indices = collect_shared_string_indices(&raw_headers);
    let shared_strings = read_xlsx_shared_strings_subset(zip, &needed_indices)?;
    let headers = raw_headers
        .into_iter()
        .map(|value| {
            value
                .map(|value| {
                    xlsx_cell_value_to_string(resolve_xlsx_cell_value(value, &shared_strings))
                })
                .unwrap_or_default()
        })
        .collect();

    Ok((headers, dimensions))
}

fn read_xlsx_shared_strings_subset(
    zip: &mut ZipArchive<BufReader<File>>,
    needed_indices: &HashSet<usize>,
) -> Result<HashMap<usize, String>, String> {
    if needed_indices.is_empty() {
        return Ok(HashMap::new());
    }

    let file = match zip.by_name("xl/sharedStrings.xml") {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(HashMap::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut xml = configured_xml_reader(file);
    let mut buf = Vec::with_capacity(1024);
    let mut strings = HashMap::new();
    let max_needed = needed_indices.iter().copied().max().unwrap_or(0);
    let mut current_idx = 0usize;

    loop {
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
            Event::Start(e) if e.local_name().as_ref() == b"si" => {
                let value = read_xlsx_shared_string_item(&mut xml)?;
                if needed_indices.contains(&current_idx) {
                    strings.insert(current_idx, value);
                    if strings.len() == needed_indices.len() {
                        break;
                    }
                }
                current_idx += 1;
                if current_idx > max_needed && strings.len() == needed_indices.len() {
                    break;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(strings)
}

fn read_xlsx_shared_string_item<R: BufRead>(xml: &mut XmlReader<R>) -> Result<String, String> {
    let mut buf = Vec::with_capacity(1024);
    let mut text = String::new();

    loop {
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
            Event::Start(e) if e.local_name().as_ref() == b"t" => {
                text.push_str(&read_xml_text_until(xml, b"t")?);
            }
            Event::End(e) if e.local_name().as_ref() == b"si" => break,
            Event::Eof => return Err("Unexpected end of sharedStrings.xml.".into()),
            _ => {}
        }
    }

    Ok(text)
}

fn configured_xml_reader<R: Read>(reader: R) -> XmlReader<BufReader<R>> {
    let mut xml = XmlReader::from_reader(BufReader::new(reader));
    let config = xml.config_mut();
    config.check_end_names = false;
    config.trim_text(false);
    config.check_comments = false;
    config.expand_empty_elements = true;
    xml
}

fn normalize_xlsx_part_path(target: &str) -> String {
    if target.starts_with("/xl/") {
        target.trim_start_matches('/').to_string()
    } else if target.starts_with("xl/") {
        target.to_string()
    } else {
        format!("xl/{target}")
    }
}

fn xml_attr_value<R: BufRead>(
    xml: &XmlReader<R>,
    element: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, String> {
    for attr in element.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        if attr.key == QName(name) {
            return attr
                .decode_and_unescape_value(xml.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|e| e.to_string());
        }
    }
    Ok(None)
}

fn read_xml_text_until<R: BufRead>(
    xml: &mut XmlReader<R>,
    end_name: &[u8],
) -> Result<String, String> {
    let mut buf = Vec::with_capacity(1024);
    let mut text = String::new();

    loop {
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
            Event::Text(value) => text.push_str(&value.unescape().map_err(|e| e.to_string())?),
            Event::CData(value) => text.push_str(&String::from_utf8_lossy(value.as_ref())),
            Event::End(e) if e.local_name().as_ref() == end_name => break,
            Event::Eof => return Err("Unexpected end of XML text node.".into()),
            _ => {}
        }
    }

    Ok(text)
}

fn read_xlsx_header_row<R: BufRead>(
    xml: &mut XmlReader<R>,
    headers: &mut Vec<Option<XlsxCellValue>>,
    base_col: usize,
) -> Result<(), String> {
    let mut buf = Vec::with_capacity(4096);

    loop {
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
            Event::Start(e) if e.local_name().as_ref() == b"c" => {
                let col = xml_attr_value(xml, &e, b"r")?
                    .and_then(|reference| parse_xlsx_cell_ref(&reference).map(|(_, col)| col))
                    .unwrap_or(base_col);
                let relative_col = col.saturating_sub(base_col);
                if relative_col >= headers.len() {
                    headers.resize(relative_col + 1, None);
                }
                headers[relative_col] = Some(read_xlsx_cell_value(xml, &e)?);
            }
            Event::End(e) if e.local_name().as_ref() == b"row" => break,
            Event::Eof => return Err("Unexpected end of worksheet header row.".into()),
            _ => {}
        }
    }

    Ok(())
}

fn read_xlsx_cell_value<R: BufRead>(
    xml: &mut XmlReader<R>,
    cell: &BytesStart<'_>,
) -> Result<XlsxCellValue, String> {
    let cell_type = xml_attr_value(xml, cell, b"t")?;
    let mut raw_value: Option<String> = None;
    let mut inline_value = String::new();
    let mut buf = Vec::with_capacity(1024);

    loop {
        buf.clear();
        match xml.read_event_into(&mut buf).map_err(|e| e.to_string())? {
            Event::Start(e) if e.local_name().as_ref() == b"v" => {
                raw_value = Some(read_xml_text_until(xml, b"v")?);
            }
            Event::Start(e) if e.local_name().as_ref() == b"t" => {
                inline_value.push_str(&read_xml_text_until(xml, b"t")?);
            }
            Event::End(e) if e.local_name().as_ref() == b"c" => break,
            Event::Eof => return Err("Unexpected end of worksheet cell.".into()),
            _ => {}
        }
    }

    if !inline_value.is_empty() {
        return Ok(XlsxCellValue::Json(serde_json::Value::String(inline_value)));
    }

    let Some(raw_value) = raw_value else {
        return Ok(XlsxCellValue::Json(serde_json::Value::Null));
    };

    match cell_type.as_deref() {
        Some("s") => {
            let idx = raw_value.parse::<usize>().unwrap_or(usize::MAX);
            Ok(XlsxCellValue::SharedString(idx))
        }
        Some("b") => Ok(XlsxCellValue::Json(serde_json::Value::Bool(
            raw_value == "1" || raw_value == "true",
        ))),
        Some("str") => Ok(XlsxCellValue::Json(serde_json::Value::String(raw_value))),
        _ => Ok(XlsxCellValue::Json(csv_field_to_json(&raw_value))),
    }
}

fn collect_shared_string_indices(values: &[Option<XlsxCellValue>]) -> HashSet<usize> {
    values
        .iter()
        .filter_map(|value| match value {
            Some(XlsxCellValue::SharedString(idx)) => Some(*idx),
            _ => None,
        })
        .collect()
}

fn resolve_xlsx_cell_value(
    value: XlsxCellValue,
    shared_strings: &HashMap<usize, String>,
) -> serde_json::Value {
    match value {
        XlsxCellValue::Json(value) => value,
        XlsxCellValue::SharedString(idx) => shared_strings
            .get(&idx)
            .map(|value| serde_json::Value::String(value.clone()))
            .unwrap_or(serde_json::Value::Null),
    }
}

fn xlsx_cell_value_to_string(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value,
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn parse_xlsx_dimension(reference: &str) -> Option<XlsxDimensions> {
    let mut parts = reference.split(':');
    let start = parts.next().and_then(parse_xlsx_cell_ref)?;
    let end = parts.next().and_then(parse_xlsx_cell_ref).unwrap_or(start);

    Some(XlsxDimensions {
        start_row: start.0.min(end.0),
        start_col: start.1.min(end.1),
        end_row: start.0.max(end.0),
        end_col: start.1.max(end.1),
    })
}

fn parse_xlsx_cell_ref(reference: &str) -> Option<(usize, usize)> {
    let mut col = 0usize;
    let mut row = 0usize;
    let mut has_col = false;
    let mut has_row = false;

    for byte in reference.bytes() {
        if byte.is_ascii_alphabetic() {
            has_col = true;
            col = col * 26 + ((byte.to_ascii_uppercase() - b'A') as usize + 1);
        } else if byte.is_ascii_digit() {
            has_row = true;
            row = row * 10 + ((byte - b'0') as usize);
        }
    }

    if has_col && has_row {
        Some((row.saturating_sub(1), col.saturating_sub(1)))
    } else {
        None
    }
}

fn csv_field_to_json(value: &str) -> serde_json::Value {
    if value.is_empty() {
        serde_json::Value::Null
    } else if let Ok(n) = value.trim().parse::<f64>() {
        serde_json::Value::from(n)
    } else {
        serde_json::Value::String(value.to_string())
    }
}

// --- Existing Full Load Command (Unused by UI) ---
#[tauri::command]
async fn load_file_fully(filepath: String) -> Result<FullDataPayload, String> {
    let start_time = Instant::now();
    info!("(Legacy) Starting full file load for: {}", filepath);
    let path = Path::new(&filepath);
    if !path.exists() {
        return Err("File does not exist.".into());
    }
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let result = if extension == "csv" {
        load_csv(&filepath)
    } else if extension == "xlsx" || extension == "xls" {
        load_excel(&filepath)
    } else {
        Err("Unsupported file format.".into())
    };
    info!(
        "(Legacy) Total processing time for {}: {:?}",
        filepath,
        start_time.elapsed()
    );
    result
}

fn load_csv(filepath: &str) -> Result<FullDataPayload, String> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_path(filepath)
        .map_err(|e| e.to_string())?;
    let headers = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(String::from)
        .collect::<Vec<String>>();
    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| e.to_string())?;
        let fields = record
            .iter()
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        rows.push(RowData { fields });
    }
    Ok(FullDataPayload {
        total_rows: rows.len(),
        headers,
        rows,
    })
}

fn load_excel(filepath: &str) -> Result<FullDataPayload, String> {
    let mut workbook: Sheets<BufReader<File>> =
        open_workbook_auto(filepath).map_err(|e| format!("Failed to open file: {}", e))?;
    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() {
        return Err("No sheets found in the workbook.".into());
    }
    let first_sheet_name = &sheet_names[0];
    let headers_range = workbook
        .worksheet_range(first_sheet_name)
        .map_err(|e| format!("Error reading sheet '{}': {}", first_sheet_name, e))?;
    let headers = headers_range
        .rows()
        .next()
        .map(|r| r.iter().map(excel_cell_to_string).collect())
        .unwrap_or_else(Vec::new);
    if headers.is_empty() {
        return Err("Could not read headers from the first sheet.".into());
    }
    let mut all_rows = Vec::new();
    for sheet_name in sheet_names.iter() {
        if let Ok(range) = workbook.worksheet_range(sheet_name) {
            all_rows.extend(range.rows().skip(1).map(|r| RowData {
                fields: r.iter().map(excel_cell_to_json).collect(),
            }));
        }
    }
    Ok(FullDataPayload {
        total_rows: all_rows.len(),
        headers,
        rows: all_rows,
    })
}

// --- Utility Functions ---
fn header_row_index(skip_first_row: bool) -> usize {
    if skip_first_row {
        1
    } else {
        0
    }
}

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
