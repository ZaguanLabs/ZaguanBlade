import { Logger } from "./logger";

/**
 * A simple counter class.
 */
export class Counter {
    count = 0;

    increment() {
        return bump(this.count);
    }
}

export const makeCounter = (start) => {
    return new Counter();
};

function bump(value) {
    return value + 1;
}
