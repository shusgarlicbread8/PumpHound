# Desktop e2e placeholders

These files describe the Playwright cases from milestone 9. They are **not wired** to `@playwright/test` yet (that package is not a desktop dependency). Install Playwright and convert `cases` into `test()` blocks when the GUI is ready for a recorded run.

Shared plan: `../../tests/e2e/PLAN.md`.

Never use live signing, seed input or bundle submit in a spec. Reject those modes.
