export interface WindowInfo {
    id: number;
    title: string;
    app_name: string;
    x: number;
    y: number;
    width: number;
    height: number;
}

export interface CaptureResult {
    data: string;
    width: number;
    height: number;
    mime_type: string;
}
