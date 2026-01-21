// Individual patch hunk within a multi-patch operation
export type PatchHunk = {
  old_text: string;
  new_text: string;
  start_line?: number;
  end_line?: number;
};

// Discriminated union for Change types
export type Change =
  // New: Multi-patch for atomic multi-hunk edits
  | {
    change_type: "multi_patch";
    id: string;
    path: string;
    patches: PatchHunk[];
    applied?: boolean;
  }
  // Legacy: Single patch
  | {
    change_type: "patch";
    id: string;
    path: string;
    old_content: string;
    new_content: string;
    applied?: boolean;
  }
  | {
    change_type: "new_file";
    id: string;
    path: string;
    content: string;
    applied?: boolean;
  }
  | {
    change_type: "delete_file";
    id: string;
    path: string;
    applied?: boolean;
  };
