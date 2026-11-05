# TODO: finish group 7 (resource browsing)

Change: `define-app-architecture`. All group 7 work is uncommitted on `workspace-foundations`.

Tasks 7.1-7.7 are verified and ticked. The workspace tests, clippy, fmt, the crate graph check, and the license check all pass.

## Verify

- [ ] Launch the app and browse a real cluster. Use a throwaway local cluster rather than a work or production context.
- [ ] Confirm that a CRD installed during a session appears in the navigation without reconnecting, on a real cluster. Covered so far by a session test and a UI test, not end to end through the runtime.
- [ ] 7.8 10-minute soak test on the reference machine (4 cores, 16 GB) for the memory limits and the 5-minute watch release. Not to be run for now. No soak program exists yet.

## Finish

- [ ] Tick 7.8 in `openspec/changes/define-app-architecture/tasks.md` once it is verified
- [ ] Commit group 7 through a Haiku subagent. The last commits are unsigned, so sign this one if 1Password is working again.
