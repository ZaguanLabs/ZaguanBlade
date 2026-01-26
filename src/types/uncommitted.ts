export interface UncommittedChange {
  id: string;
  file_path: string;
  snapshot_id: string;
  unified_diff: string;
  added_lines: number;
  removed_lines: number;
  timestamp: number;
}
