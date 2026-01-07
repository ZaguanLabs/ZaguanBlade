export interface EphemeralDocument {
  id: string;
  content: string;
  suggested_name: string;
  created_at: string;
  modified: boolean;
}

export interface DocumentTab {
  id: string;
  title: string;
  content: string;
  isEphemeral: boolean;
  isDirty: boolean;
  suggestedName?: string;
}
