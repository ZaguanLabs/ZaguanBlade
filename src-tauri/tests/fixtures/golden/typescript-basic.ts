import { Logger } from "./logger";

/// A greeter interface.
export interface Greeter {
    greet(name: string): string;
}

export enum Status {
    Active,
    Inactive,
}

export class UserService {
    private count: number = 0;

    greet(name: string): string {
        return helper(name);
    }
}

export const authenticate = (token: string): boolean => {
    return token.length > 0;
};

function helper(name: string): string {
    return name.trim();
}

// Reads the PORT environment variable (M4.2 reads_env fixture). `process.env.PORT`
// is a member access, so the KEY is the property name and there is no call edge.
export function loadPort(): string {
    return process.env.PORT;
}

// NEGATIVE reads_env fixture (M4.2 anchoring): `foo.process.env.PORT` is NOT the
// global `process.env` (the `.env` member's object is a nested member access, not
// the bare `process` identifier), so this must NOT emit a reads_env edge.
export function readNested(foo: any): string {
    return foo.process.env.PORT;
}
