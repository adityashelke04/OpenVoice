/// <reference types="vite/client" />

/** Replaced at build time by the `define` in `vite.config.ts`. It is genuinely
 *  absent under vitest, which loads `vitest.config.ts` and its own empty
 *  `define`, so every read goes through a `typeof` guard. */
declare const __APP_VERSION__: string | undefined;
