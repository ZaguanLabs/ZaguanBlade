export function centerElementInScrollContainer(container: HTMLElement, element: HTMLElement) {
    const containerRect = container.getBoundingClientRect();
    const elementRect = element.getBoundingClientRect();
    const elementTop = elementRect.top - containerRect.top + container.scrollTop;
    const centeredTop = elementTop - (container.clientHeight - elementRect.height) / 2;
    const maxScrollTop = container.scrollHeight - container.clientHeight;

    container.scrollTop = Math.max(0, Math.min(centeredTop, maxScrollTop));
}

export function findModelElement(container: HTMLElement, modelId: string) {
    return Array.from(container.querySelectorAll<HTMLElement>('[data-model-id]'))
        .find(element => element.dataset.modelId === modelId) || null;
}
