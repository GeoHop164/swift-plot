export interface RowData {
  fields: string[];
}

export interface ParsedFileResult {
  headers: string[];
  rows: RowData[];
}
