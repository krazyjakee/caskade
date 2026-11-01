export declare const API: string;
export declare function validId(id: string): boolean;
export interface GenerateOptions {
  id: string;
  name: string;
  ui: boolean;
  backend: boolean;
  /** Must not exist or be empty. */
  dir: string;
  /** A Caskade checkout to link the unpublished SDK packages from. */
  sdk?: string;
}
/** Writes the plugin; returns the files, relative to `dir`. */
export declare function generate(options: GenerateOptions): string[];
