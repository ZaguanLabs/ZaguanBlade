export interface McpElicitationChoice { value: string; label: string }

export type McpElicitationField = { name: string; title: string | null; description: string | null; required: boolean } & (
    | { kind: 'text'; min_length: number | null; max_length: number; format: 'email' | 'uri' | 'date' | 'date-time' | null; default: string | null }
    | { kind: 'number'; integer: boolean; minimum: number | null; maximum: number | null; default: number | null }
    | { kind: 'boolean'; default: boolean | null }
    | { kind: 'choice'; options: McpElicitationChoice[]; multiple: boolean; min_items: number | null; max_items: number | null; default: string[] }
);

export type McpElicitation = { request_id: string; server_name: string; tool_name: string; message: string } & (
    | { mode: 'form'; fields: McpElicitationField[] }
    | { mode: 'url'; url: string; host: string }
);

/** What each input holds while the user edits it. */
export type FieldValue = string | boolean | string[];

export function initialValues(fields: McpElicitationField[]): Record<string, FieldValue> {
    return Object.fromEntries(fields.map(field => {
        switch (field.kind) {
            case 'text': return [field.name, field.default ?? ''];
            case 'number': return [field.name, field.default === null ? '' : String(field.default)];
            case 'boolean': return [field.name, field.default ?? false];
            case 'choice': return [field.name, field.multiple ? field.default : field.default[0] ?? ''];
        }
    }));
}

/** Content for an accept, or the names of fields that are missing or invalid.
 * Empty optional fields are omitted. The backend validates again. */
export function elicitationContent(fields: McpElicitationField[], values: Record<string, FieldValue>):
    { content: Record<string, unknown>; invalid: string[] } {
    const content: Record<string, unknown> = {};
    const invalid: string[] = [];
    for (const field of fields) {
        const value = values[field.name];
        const empty = value === '' || value === undefined || (Array.isArray(value) && value.length === 0 && field.kind === 'choice' && !field.min_items);
        if (field.kind !== 'boolean' && empty) {
            if (field.required) invalid.push(field.name);
            continue;
        }
        switch (field.kind) {
            case 'text': {
                const text = String(value);
                const length = [...text].length;
                if (length > field.max_length || (field.min_length !== null && length < field.min_length)) invalid.push(field.name);
                else content[field.name] = text;
                break;
            }
            case 'number': {
                const number = Number(String(value).trim());
                if (!Number.isFinite(number) || (field.integer && !Number.isInteger(number))
                    || (field.minimum !== null && number < field.minimum) || (field.maximum !== null && number > field.maximum)) invalid.push(field.name);
                else content[field.name] = number;
                break;
            }
            case 'boolean':
                content[field.name] = value === true;
                break;
            case 'choice': {
                const items = Array.isArray(value) ? value : [value];
                if (field.multiple && ((field.min_items !== null && items.length < field.min_items) || (field.max_items !== null && items.length > field.max_items))) invalid.push(field.name);
                else content[field.name] = field.multiple ? items : items[0];
                break;
            }
        }
    }
    return { content, invalid };
}
