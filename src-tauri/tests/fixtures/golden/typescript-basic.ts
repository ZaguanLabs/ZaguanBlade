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
