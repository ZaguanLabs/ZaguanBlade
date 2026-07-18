/** Builds the navigation toolbar model shown in the page header. */
export function buildNavigationToolbar(labels: string[]): string[] {
  return labels.map((label) => label.trim()).filter((label) => label.length > 0);
}
