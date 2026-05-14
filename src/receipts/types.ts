export interface Receipt {
  id: number;
  sessionId: number | null;
  cwd: string;
  date: string;
  createdAt: number;
  updatedAt: number;
}
