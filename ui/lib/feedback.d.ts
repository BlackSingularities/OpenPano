export interface FeedbackOptions {
  app: string;
  appName: string;
  version: string | (() => string | Promise<string>);
  lang?: string | (() => string);
  platform?: string;
  offerCrash?: boolean;
}
export function initFeedback(options: FeedbackOptions): void;
export function openFeedback(kind?: "bug" | "feature"): void;
export function offerPendingCrash(): void;
export function reportError(error: unknown): void;
