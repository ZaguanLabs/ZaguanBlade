// Discriminated union for Change types
export type Change =
  | {
      change_type: "patch";
      id: string;
      path: string;
      old_content: string;
      new_content: string;
    }
  | {
      change_type: "new_file";
      id: string;
      path: string;
      content: string;
    }
  | {
      change_type: "delete_file";
      id: string;
      path: string;
    };
