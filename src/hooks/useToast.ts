export type ToastProps = {
    title?: string;
    description?: string;
    variant?: "default" | "destructive";
};

export function useToast() {
    const toast = (props: ToastProps) => {
        console.log(`[TOAST] ${props.title}: ${props.description}`);
    };

    return { toast };
}
