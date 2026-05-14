export function shouldDetachChatAutoScrollOnWheel(deltaY: number, scrollTop: number) {
    return deltaY < 0 && scrollTop > 0;
}

export function isNearChatBottom(scrollHeight: number, scrollTop: number, clientHeight: number, thresholdPx: number) {
    return Math.abs(scrollHeight - scrollTop - clientHeight) < thresholdPx;
}
