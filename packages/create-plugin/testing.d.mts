export declare function tempFolder(): string;
export declare function readText(dir: string, path: string): string;
export declare function removeFolder(dir: string): void;
export declare function joinPath(...parts: string[]): string;
/** The names in a folder, sorted. */
export declare function listFolder(dir: string, path: string): string[];
