# TODO: finish group 7 (resource browsing)

Change: `define-app-architecture`. Tasks 7.1-7.7 are verified, ticked, and committed.

## Verify

- [ ] Browse a throwaway minikube cluster by hand in the app. `real_cluster` covers the same flows through the runtime, host, and store, but nobody has clicked through the window or seen the delete dialog on screen.
- [ ] 7.8 Run `cargo run -p kubepulse-ui --release --example soak` on the reference machine (4 cores, 16 GB, integrated GPU), then tick 7.8 in `openspec/changes/define-app-architecture/tasks.md`. A run pinned to 4 cores on the development machine, which has a discrete GPU, peaked at 304 MiB and released the watch after 300 seconds.
