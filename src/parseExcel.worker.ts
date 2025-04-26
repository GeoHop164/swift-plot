// parseExcel.worker.ts
import { read, utils } from 'xlsx';

self.onmessage = async (event) => {
  const { fileBuffer } = event.data;

  const workbook = read(fileBuffer, { type: 'array' });
  const worksheet = workbook.Sheets[workbook.SheetNames[0]];
  const jsonData = utils.sheet_to_json(worksheet);

  self.postMessage({ jsonData });
};
