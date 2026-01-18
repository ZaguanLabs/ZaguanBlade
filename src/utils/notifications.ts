import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';

/**
 * Notification utility for ZaguanBlade
 * Provides a simple interface for sending system notifications
 */

let permissionGranted: boolean | null = null;

/**
 * Initialize notification permissions
 * Call this once during app startup
 */
export async function initNotifications(): Promise<boolean> {
    try {
        permissionGranted = await isPermissionGranted();
        
        if (!permissionGranted) {
            const permission = await requestPermission();
            permissionGranted = permission === 'granted';
        }
        
        return permissionGranted;
    } catch (err) {
        console.error('[Notifications] Failed to initialize:', err);
        return false;
    }
}

/**
 * Send a notification to the user
 * @param title - Notification title
 * @param body - Notification body text
 * @param options - Optional notification options
 */
export async function notify(
    title: string,
    body: string,
    options?: {
        icon?: string;
        sound?: string;
    }
): Promise<void> {
    try {
        // Check permission if not already checked
        if (permissionGranted === null) {
            await initNotifications();
        }

        if (!permissionGranted) {
            console.warn('[Notifications] Permission not granted, skipping notification');
            return;
        }

        await sendNotification({
            title,
            body,
            ...options
        });
    } catch (err) {
        console.error('[Notifications] Failed to send notification:', err);
    }
}

/**
 * Notify about file changes detected
 * @param fileCount - Number of files changed
 * @param fileNames - Optional array of changed file names
 */
export async function notifyFileChanges(
    fileCount: number,
    fileNames?: string[]
): Promise<void> {
    const title = 'Files Changed';
    let body = `${fileCount} file${fileCount > 1 ? 's' : ''} modified in workspace`;
    
    if (fileNames && fileNames.length > 0) {
        const displayFiles = fileNames.slice(0, 3);
        body = displayFiles.join(', ');
        if (fileNames.length > 3) {
            body += ` and ${fileNames.length - 3} more`;
        }
    }
    
    await notify(title, body);
}

/**
 * Notify about build completion
 * @param success - Whether the build succeeded
 * @param duration - Build duration in seconds
 */
export async function notifyBuildComplete(
    success: boolean,
    duration?: number
): Promise<void> {
    const title = success ? 'Build Succeeded' : 'Build Failed';
    let body = success ? 'Your build completed successfully' : 'Build encountered errors';
    
    if (duration) {
        body += ` (${duration.toFixed(1)}s)`;
    }
    
    await notify(title, body);
}

/**
 * Notify about task completion
 * @param taskName - Name of the completed task
 * @param message - Optional additional message
 */
export async function notifyTaskComplete(
    taskName: string,
    message?: string
): Promise<void> {
    const title = 'Task Complete';
    const body = message || `${taskName} has finished`;
    
    await notify(title, body);
}
