import { StateField, StateEffect } from "@codemirror/state";
import { EditorView, Decoration, DecorationSet, WidgetType } from "@codemirror/view";
import { createRoot } from "react-dom/client";
import React from "react";
import { DiffWidget } from "../DiffWidget";

// Data Model
export interface DiffBlock {
    id: string;
    from: number;
    to: number;
    original: string;
    modified: string;
}

// Effects
export const addDiff = StateEffect.define<DiffBlock>();
export const removeDiff = StateEffect.define<string>();
export const clearDiffs = StateEffect.define<null>();
export const acceptDiff = StateEffect.define<string>();
export const rejectDiff = StateEffect.define<string>();

// Widget
class DiffWidgetType extends WidgetType {
    constructor(readonly diff: DiffBlock) { super(); }

    eq(other: DiffWidgetType) {
        return other.diff.id === this.diff.id;
    }

    toDOM(view: EditorView) {
        const dom = document.createElement("div");
        const root = createRoot(dom);

        const onAccept = () => {
            // Calculate current position dynamically
            const from = view.posAtDOM(dom);
            const len = this.diff.to - this.diff.from;
            const to = from + len;

            view.dispatch({
                changes: { from, to, insert: this.diff.modified },
                effects: removeDiff.of(this.diff.id)
            });
        };

        const onReject = () => {
            view.dispatch({
                effects: removeDiff.of(this.diff.id)
            });
        };

        root.render(
            <DiffWidget
                original={this.diff.original}
                modified={this.diff.modified}
                onAccept={onAccept}
                onReject={onReject}
            />
        );
        return dom;
    }
}

// State Field
export const diffsField = StateField.define<DecorationSet>({
    create() { return Decoration.none; },
    update(diffs, tr) {
        // Map existing decorations forward through changes
        diffs = diffs.map(tr.changes);

        for (const e of tr.effects) {
            if (e.is(addDiff)) {
                const deco = Decoration.replace({
                    widget: new DiffWidgetType(e.value),
                    block: true,
                });
                diffs = diffs.update({ add: [deco.range(e.value.from, e.value.to)] });
            } else if (e.is(removeDiff)) {
                diffs = diffs.update({
                    filter: (from, to, value) => {
                        return !(value.spec.widget instanceof DiffWidgetType && value.spec.widget.diff.id === e.value);
                    }
                });
            } else if (e.is(clearDiffs)) {
                diffs = Decoration.none;
            }
        }
        return diffs;
    },
    provide: f => EditorView.decorations.from(f)
});
