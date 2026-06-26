# Contributing

This project is an early community Rust implementation aligned with the Lark/Feishu Channel SDK family. The repository is still stabilizing, so contribution rules are intentionally lightweight.

## Branches

Use short typed branch prefixes:

- `feat/<name>` for new functionality
- `fix/<name>` for bug fixes
- `docs/<name>` for documentation-only changes
- `ci/<name>` for workflow changes
- `refactor/<name>` for behavior-preserving code changes
- `release/<version>` for release preparation

The default branch is `main`. Feature work should happen through pull requests.

## Commits

Use Conventional Commit-style messages:

- `feat: add app token client`
- `fix: handle api error payloads`
- `docs: update roadmap`
- `ci: add Rust workflow`
- `refactor: split outbound message types`

Keep commits focused. Prefer follow-up commits during draft review over force-pushing away useful history, unless the branch has not been reviewed yet.

Draft PRs may collect multiple review and iteration commits. Before marking a PR ready for review, squash or fix up the branch into one clear commit. The CI workflow checks this shape for non-draft pull requests.

After the branch has one commit, merge the PR with GitHub's **Create a merge commit** option. This keeps `main` history readable while preserving the branch topology in the commit graph.

To clean up a branch before review, either use interactive rebase:

```bash
git fetch origin
git rebase -i origin/main
git push --force-with-lease
```

Or, when the whole branch should become one commit, recreate it from the current diff:

```bash
git fetch origin
git reset --soft origin/main
git commit -m "feat: describe the change"
git push --force-with-lease
```

## Pull Requests

Open draft PRs for larger work while the shape is still changing. Mark a PR ready for review when:

- the implemented scope is clear
- README/docs match the current behavior
- CI passes
- new behavior has tests or a clear reason tests are deferred

PR descriptions should include:

- summary of changes
- what is intentionally out of scope
- verification commands, usually `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features`

## Releases

Do not create git tags or publish to crates.io until the crate has a working API surface, examples, tests, and a documented compatibility policy.
